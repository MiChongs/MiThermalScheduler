//! Joyose（小米游戏加速服务）调教
//!
//! Joyose 启动及云控更新时，会从 4 个来源（云控 SDK、系统设置、SmartP.db、本地默认配置）
//! 收集 booster_config，按 priority（缺省 5）与 version 取胜者交给解析器；解析器只接受
//! 版本号高于当前已加载版本（仅存于内存）的配置。
//!
//! 因此调教方式为：以最新的官方配置为基线生成调教版，写入 SmartP.db 并赋予远高于官方
//! 日期型版本号的版本，然后重启 Joyose 使其重新选择并加载。还原时写回备份的官方记录并
//! 再次重启。守护进程定期复核，记录被同步覆盖或官方配置更新时重新生成。

use crate::profile::{JoyCpuMode, JoyFpsMode, JoyoseConfig};
use crate::{logln, paths};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

pub const PKG: &str = "com.xiaomi.joyose";
const SMARTP: &str = "/data/data/com.xiaomi.joyose/databases/SmartP.db";
const TEG: &str = "/data/data/com.xiaomi.joyose/databases/teg_config.db";
const CONFIG_NAME: &str = "booster_config";
/// 注入版本号：Joyose 以 int 读取版本，需小于 2^31，且远高于官方日期型版本（如 2026042851）
pub const INJECT_VERSION: i64 = 2_147_483_000;

fn backup_path() -> String {
    format!("{}/joyose_official.json", paths::DATA)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Row {
    pub group_name: Option<String>,
    pub enable: i64,
    pub version: i64,
    pub with_model: i64,
    pub model: Option<String>,
    pub params: String,
}

pub fn installed() -> bool {
    fs::metadata(SMARTP).is_ok()
}

fn open(flags: OpenFlags, path: &str) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX).with_context(|| format!("无法打开 {path}"))?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}

fn read_row(conn: &Connection) -> Result<Option<Row>> {
    Ok(conn
        .query_row(
            "SELECT group_name, enable, version, with_model, model, params FROM cloud_config WHERE config_name = ?1",
            params![CONFIG_NAME],
            |r| {
                Ok(Row {
                    group_name: r.get(0)?,
                    enable: r.get::<_, Option<i64>>(1)?.unwrap_or(1),
                    version: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    with_model: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    model: r.get(4)?,
                    params: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                })
            },
        )
        .optional()?)
}

fn write_row(conn: &Connection, row: &Row) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO cloud_config (config_name, group_name, enable, version, with_model, model, params) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![CONFIG_NAME, row.group_name, row.enable, row.version, row.with_model, row.model, row.params],
    )?;
    Ok(())
}

/// 云控 SDK 缓存（teg_config.db）中的 booster_config
fn teg_rows() -> Vec<Row> {
    let Ok(conn) = open(OpenFlags::SQLITE_OPEN_READ_ONLY, TEG) else { return Vec::new() };
    let Ok(mut stmt) = conn.prepare("SELECT rule_content FROM rules WHERE rule_module = ?1") else { return Vec::new() };
    let Ok(rows) = stmt.query_map(params![CONFIG_NAME], |r| r.get::<_, String>(0)) else { return Vec::new() };
    rows.flatten()
        .filter_map(|text| {
            let v: Value = serde_json::from_str(&text).ok()?;
            Some(Row {
                group_name: v.get("group_name").and_then(Value::as_str).map(String::from),
                enable: 1,
                version: v.get("version")?.as_i64()?,
                with_model: 0,
                model: None,
                params: v.get("params")?.to_string(),
            })
        })
        .collect()
}

fn load_backup() -> Option<Row> {
    serde_json::from_str(&fs::read_to_string(backup_path()).ok()?).ok()
}

fn save_backup(row: &Row) -> Result<()> {
    fs::create_dir_all(paths::DATA)?;
    fs::write(backup_path(), serde_json::to_vec(row)?)?;
    Ok(())
}

/// 当前可获得的最新官方配置：SmartP 原记录（或其备份）与云控 SDK 缓存中版本最高者
fn newest_official(current: Option<&Row>) -> Option<Row> {
    let mut cands: Vec<Row> = teg_rows();
    if let Some(b) = load_backup() {
        cands.push(b);
    }
    if let Some(r) = current.filter(|r| r.version != INJECT_VERSION) {
        cands.push(r.clone());
    }
    cands.into_iter().filter(|r| serde_json::from_str::<Value>(&r.params).is_ok()).max_by_key(|r| r.version)
}

/// 写入后修正属主与 SELinux 标签，避免 Joyose（system 用户）无法访问 root 新建的日志文件
fn fix_perms(owner: (u32, u32)) {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let p = format!("{SMARTP}{suffix}");
        if fs::metadata(&p).is_err() {
            continue;
        }
        if let Ok(c) = CString::new(p.clone()) {
            unsafe {
                libc::chown(c.as_ptr(), owner.0, owner.1);
            }
        }
        let _ = Command::new("restorecon").arg(&p).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

fn joyose_pids() -> Vec<i32> {
    let Ok(rd) = fs::read_dir("/proc") else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<i32>().ok())
        .filter(|pid| {
            fs::read(format!("/proc/{pid}/cmdline"))
                .map(|c| c.split(|b| *b == 0).next().map(|n| n == PKG.as_bytes()).unwrap_or(false))
                .unwrap_or(false)
        })
        .collect()
}

/// 结束 Joyose 进程；系统会在数秒内自动重启它，启动时重新选择并加载 booster_config
pub fn restart() -> bool {
    let old = joyose_pids();
    for pid in &old {
        unsafe {
            libc::kill(*pid, libc::SIGTERM);
        }
    }
    for _ in 0..40 {
        sleep(Duration::from_millis(250));
        let now = joyose_pids();
        if !now.is_empty() && now.iter().all(|p| !old.contains(p)) {
            return true;
        }
    }
    let _ = Command::new("am")
        .args(["startservice", "-n", &format!("{PKG}/.smartop.SmartOpService")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    !joyose_pids().is_empty()
}

/// 重启后等待 Joyose 加载指定版本（最多约 8 秒）
fn wait_loaded(expect: i64) {
    for _ in 0..16 {
        if loaded_version() == Some(expect) {
            return;
        }
        sleep(Duration::from_millis(500));
    }
}

/// Joyose 当前已加载的 booster_config 版本（取自 dumpsys 的 HeaderVersionCode；
/// 「Cloud Version」是配置内部 header 中的版本字符串，调教版沿用官方值，不能用来判断）
pub fn loaded_version() -> Option<i64> {
    let out = Command::new("dumpsys").args(["activity", "service", PKG]).stderr(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("BoosterCloud HeaderVersionCode is"))?;
    line.rsplit(' ').next()?.trim().parse().ok()
}

/// SmartP.db 中是否为注入的调教记录
pub fn injected() -> bool {
    open(OpenFlags::SQLITE_OPEN_READ_ONLY, SMARTP).ok().and_then(|c| read_row(&c).ok().flatten()).map(|r| r.version == INJECT_VERSION).unwrap_or(false)
}

/// 同步调教状态。返回 Some(说明) 表示发生了注入或还原
pub fn sync(cfg: &JoyoseConfig, disabled: bool) -> Result<Option<String>> {
    if !installed() {
        return Ok(None);
    }
    let meta = fs::metadata(SMARTP)?;
    let owner = (meta.uid(), meta.gid());
    let conn = open(OpenFlags::SQLITE_OPEN_READ_WRITE, SMARTP)?;
    let current = read_row(&conn)?;
    let ours = current.as_ref().map(|r| r.version == INJECT_VERSION).unwrap_or(false);

    let official = newest_official(current.as_ref());
    let tuned = match (&official, cfg.enabled && !disabled) {
        (Some(o), true) => {
            let t = tune(&o.params, cfg)?;
            (t != canonical(&o.params)).then_some(t)
        }
        _ => None,
    };

    let Some(tuned) = tuned else {
        if !ours {
            return Ok(None);
        }
        // 还原官方记录
        match load_backup() {
            Some(b) => write_row(&conn, &b)?,
            None => {
                conn.execute("DELETE FROM cloud_config WHERE config_name = ?1", params![CONFIG_NAME])?;
            }
        }
        let restored = official.as_ref().map(|o| o.version);
        drop(conn);
        fix_perms(owner);
        let _ = fs::remove_file(backup_path());
        restart();
        if let Some(v) = restored {
            wait_loaded(v);
        }
        logln!("Joyose 调教已还原为官方配置");
        return Ok(Some("已还原官方配置".into()));
    };

    let official = official.unwrap();
    if let Some(r) = current.as_ref().filter(|_| !ours) {
        save_backup(r)?;
    }
    if ours && current.as_ref().map(|r| r.params == tuned).unwrap_or(false) {
        return Ok(None);
    }
    write_row(
        &conn,
        &Row {
            group_name: official.group_name.clone().or_else(|| Some(CONFIG_NAME.into())),
            enable: 1,
            version: INJECT_VERSION,
            with_model: 0,
            model: official.model.clone(),
            params: tuned,
        },
    )?;
    drop(conn);
    fix_perms(owner);
    restart();
    wait_loaded(INJECT_VERSION);
    logln!("Joyose 调教已注入（官方基线版本 {}）", official.version);
    Ok(Some(format!("已注入（官方基线 {}）", official.version)))
}

/// 状态摘要，供 WebUI 展示
pub fn status(cfg: &JoyoseConfig) -> Value {
    if !installed() {
        return json!({ "installed": false });
    }
    let current = open(OpenFlags::SQLITE_OPEN_READ_ONLY, SMARTP).ok().and_then(|c| read_row(&c).ok().flatten());
    let official = newest_official(current.as_ref());
    let injected = current.as_ref().map(|r| r.version == INJECT_VERSION).unwrap_or(false);
    let loaded = loaded_version();
    let off_params = official.as_ref().map(|o| o.params.clone()).unwrap_or_default();
    let preview_cfg = JoyoseConfig { enabled: true, ..cfg.clone() };
    let tuned = tune(&off_params, &preview_cfg).unwrap_or_default();
    json!({
        "installed": true,
        "injected": injected,
        "active": injected && loaded == Some(INJECT_VERSION),
        "loadedVersion": loaded,
        "officialVersion": official.as_ref().map(|o| o.version),
        "rowVersion": current.as_ref().map(|r| r.version),
        "official": summary(&off_params),
        "tuned": summary(&tuned),
    })
}

// ---------------------------------------------------------------------------
// 配置变换
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    cfg: &'a JoyoseConfig,
    /// Joyose 自身的「不限制」CPU 上限（取自游戏退出时的恢复命令）
    full: Vec<u64>,
}

fn canonical(params: &str) -> String {
    serde_json::from_str::<Value>(params).map(|v| v.to_string()).unwrap_or_else(|_| params.to_string())
}

pub fn tune(params: &str, cfg: &JoyoseConfig) -> Result<String> {
    let mut root: Value = serde_json::from_str(params).context("booster_config 不是有效的 JSON")?;
    let Some(gb) = root.get_mut("game_booster") else { bail!("booster_config 缺少 game_booster") };
    let ctx = Ctx { cfg, full: full_caps(gb) };
    walk_object(gb, &ctx, false);
    Ok(root.to_string())
}

/// 从前台场景的 end（退出恢复）命令中取出 MA 恢复值
fn full_caps(gb: &Value) -> Vec<u64> {
    let scenes = gb.pointer("/booster_config/scene_config").and_then(Value::as_array);
    for scene in scenes.into_iter().flatten() {
        for b in scene.get("end").and_then(Value::as_array).into_iter().flatten() {
            if let Some(cmd) = b.get("cmd").and_then(Value::as_str) {
                if let Some(v) = parse_ma(cmd).into_iter().next() {
                    return v;
                }
            }
        }
    }
    Vec::new()
}

fn parse_ma(cmd: &str) -> Vec<Vec<u64>> {
    let mut out = Vec::new();
    let mut rest = cmd;
    while let Some(i) = rest.find("MA#") {
        let tail = &rest[i + 3..];
        let end = tail.find(|c: char| !c.is_ascii_digit() && c != ',').unwrap_or(tail.len());
        out.push(tail[..end].split(',').filter_map(|x| x.parse().ok()).collect());
        rest = &tail[end..];
    }
    out
}

/// 按模式抬高命令中的 MA（游戏 CPU 频率上限）
fn raise_ma(cmd: &str, ctx: &Ctx) -> String {
    if ctx.cfg.cpu_mode == JoyCpuMode::Stock || !cmd.contains("MA#") {
        return cmd.to_string();
    }
    let mut out = String::with_capacity(cmd.len());
    let mut rest = cmd;
    while let Some(i) = rest.find("MA#") {
        out.push_str(&rest[..i + 3]);
        let tail = &rest[i + 3..];
        let end = tail.find(|c: char| !c.is_ascii_digit() && c != ',').unwrap_or(tail.len());
        let vals: Vec<String> = tail[..end]
            .split(',')
            .enumerate()
            .map(|(idx, x)| match x.parse::<u64>() {
                Ok(v) => {
                    let target = match ctx.cfg.cpu_mode {
                        JoyCpuMode::Max => ctx.full.get(idx).copied().unwrap_or(v),
                        _ => ctx.cfg.cpu_floor.get(idx).copied().unwrap_or(0),
                    };
                    v.max(target).to_string()
                }
                Err(_) => x.to_string(),
            })
            .collect();
        out.push_str(&vals.join(","));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn fmt_temp(t: f64) -> String {
    if (t - t.round()).abs() < 1e-6 { format!("{}", t.round() as i64) } else { format!("{:.1}", t) }
}

/// 温度门限：≥30°C 的才视为温控门限（10:0 这类起始项保持不变）
fn shift_temp(key: &str, ctx: &Ctx) -> String {
    match key.trim().parse::<f64>() {
        Ok(t) if t >= 30.0 && ctx.cfg.fps_mode == JoyFpsMode::Relax => fmt_temp(t + ctx.cfg.fps_offset),
        _ => key.to_string(),
    }
}

/// 忽略单引号内逗号的切分
fn split_items(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut start, mut quoted) = (0, false);
    for (i, c) in s.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ',' if !quoted => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Table {
    /// 温度:帧率上限
    Fps,
    /// 温度:PID 参数（首个数值为目标温度）
    Pid,
}

fn map_tv(body: &str, table: Table, ctx: &Ctx) -> String {
    split_items(body)
        .into_iter()
        .map(|item| {
            let Some((k, v)) = item.split_once(':') else { return item.to_string() };
            let key = shift_temp(k, ctx);
            let val = match table {
                Table::Fps if ctx.cfg.fps_mode == JoyFpsMode::Off && k.trim().parse::<f64>().map(|t| t >= 30.0).unwrap_or(false) => "0".to_string(),
                Table::Pid => {
                    let mut parts = v.splitn(2, ' ');
                    let first = parts.next().unwrap_or("");
                    let first = shift_temp(first, ctx);
                    match parts.next() {
                        Some(rest) => format!("{first} {rest}"),
                        None => first,
                    }
                }
                _ => v.to_string(),
            };
            format!("{key}:{val}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// 形如 "120#10:0,44:90;90#10:0" 的分档表
fn map_hash(s: &str, table: Table, ctx: &Ctx) -> String {
    s.split(';')
        .map(|seg| match seg.split_once('#') {
            Some((fps, body)) => format!("{fps}#{}", map_tv(body, table, ctx)),
            None => seg.to_string(),
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// 形如 "60@42:glk#MA#...$44:glk#MA#..." 的按温度执行命令
fn map_temp_cmd(s: &str, ctx: &Ctx) -> String {
    s.split(';')
        .map(|seg| {
            let (head, body) = match seg.split_once('@') {
                Some((h, b)) => (format!("{h}@"), b),
                None => (String::new(), seg),
            };
            let items: Vec<String> = body
                .split('$')
                .map(|it| match it.split_once(':') {
                    Some((t, cmd)) => format!("{}:{}", shift_temp(t, ctx), raise_ma(cmd, ctx)),
                    None => it.to_string(),
                })
                .collect();
            format!("{head}{}", items.join("$"))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// 形如 "0:glk#MA#...$1:glk#MA#..." 的按限帧档位执行命令；df=1 为特殊档位，保持不变
fn map_df_cmd(s: &str, ctx: &Ctx) -> String {
    s.split('$')
        .map(|it| match it.split_once(':') {
            Some((df, cmd)) if df.trim() != "1" => format!("{df}:{}", raise_ma(cmd, ctx)),
            _ => it.to_string(),
        })
        .collect::<Vec<_>>()
        .join("$")
}

fn is_fps_table(k: &str) -> bool {
    matches!(k, "dynamic_fps" | "dynamic_fps_M" | "dynamic_fps_multiWin" | "dynamic_fps_multiWin_M")
}

fn is_fps_hash(k: &str) -> bool {
    k.starts_with("dynamic_") && k.contains("targetfps") && !k.contains("cpufreq")
}

fn transform_str(key: &str, s: &str, ctx: &Ctx, in_end: bool) -> String {
    let fps = ctx.cfg.fps_mode != JoyFpsMode::Stock;
    if fps && is_fps_table(key) {
        return map_tv(s, Table::Fps, ctx);
    }
    if fps && is_fps_hash(key) {
        return map_hash(s, Table::Fps, ctx);
    }
    if fps && key.starts_with("PID_") {
        return map_hash(s, Table::Pid, ctx);
    }
    if key.starts_with("execute_cmd_by_temp_") {
        return map_temp_cmd(s, ctx);
    }
    if key.starts_with("execute_cmd_by_df_") {
        return map_df_cmd(s, ctx);
    }
    if key == "glk_max_change_temp" && ctx.cfg.fps_mode == JoyFpsMode::Relax {
        return shift_temp(s, ctx);
    }
    if key == "cmd" && !in_end {
        return raise_ma(s, ctx);
    }
    s.to_string()
}

fn walk_object(v: &mut Value, ctx: &Ctx, in_end: bool) {
    let Value::Object(map) = v else { return };
    if ctx.cfg.fps_mode == JoyFpsMode::Off {
        // 关闭温控限帧：移除 PID 控制，交由（已清零的）查表路径
        let pid: Vec<String> = map.keys().filter(|k| k.starts_with("PID_")).cloned().collect();
        for k in pid {
            map.remove(&k);
        }
    }
    let keys: Vec<String> = map.keys().cloned().collect();
    for k in keys {
        let child_end = in_end || k == "end";
        let val = map.get_mut(&k).unwrap();
        walk_value(&k, val, ctx, child_end);
    }
}

fn walk_value(key: &str, v: &mut Value, ctx: &Ctx, in_end: bool) {
    match v {
        Value::String(s) => {
            if key == "cgame_df" {
                *s = map_cgame(s, ctx);
            } else {
                let new = transform_str(key, s, ctx, in_end);
                *s = new;
            }
        }
        Value::Number(n) if key == "glk_max_change_temp" && ctx.cfg.fps_mode == JoyFpsMode::Relax => {
            if let Some(t) = n.as_f64() {
                let shifted = if t >= 30.0 { t + ctx.cfg.fps_offset } else { t };
                *v = serde_json::Number::from_f64(shifted).map(Value::Number).unwrap_or(Value::Null);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                walk_value(key, item, ctx, in_end);
            }
        }
        Value::Object(_) => walk_object(v, ctx, in_end),
        _ => {}
    }
}

/// "包名@帧率档位#温度:上限,..;..."
fn map_cgame(s: &str, ctx: &Ctx) -> String {
    if ctx.cfg.fps_mode == JoyFpsMode::Stock {
        return s.to_string();
    }
    match s.split_once('@') {
        Some((pkg, rest)) => format!("{pkg}@{}", map_hash(rest, Table::Fps, ctx)),
        None => s.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 摘要
// ---------------------------------------------------------------------------

fn find_override<'a>(gb: &'a Value, name: &str) -> Option<&'a Map<String, Value>> {
    gb.pointer("/booster_config/ovrride_config")?.as_array()?.iter().filter_map(Value::as_object).find(|o| o.get("game_name").and_then(Value::as_str) == Some(name))
}

/// 关键参数摘要：王者荣耀温控限帧与 PID、全局限帧表、通用游戏 CPU 上限
pub fn summary(params: &str) -> Value {
    let Ok(root) = serde_json::from_str::<Value>(params) else { return Value::Null };
    let Some(gb) = root.get("game_booster") else { return Value::Null };
    let str_at = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or("").to_string();
    let dfg = gb.get("dynamic_fps_global").map(|v| match v {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    });
    let fg_ma = gb
        .pointer("/booster_config/scene_config")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|s| s.get("scene_id").and_then(Value::as_i64) == Some(10001)))
        .and_then(|s| s.get("booster")?.as_array()?.iter().find_map(|b| parse_ma(b.get("cmd")?.as_str()?).into_iter().next()))
        .unwrap_or_default();
    let sgame = find_override(gb, "SGAME");
    let pid120 = sgame.and_then(|o| o.get("PID_T")).and_then(Value::as_str).and_then(|s| s.split(';').find(|x| x.starts_with("120#")).map(String::from)).unwrap_or_default();
    let df_ma: Map<String, Value> = sgame
        .and_then(|o| o.get("execute_cmd_by_df_T"))
        .and_then(Value::as_str)
        .map(|s| {
            s.split('$')
                .filter_map(|it| {
                    let (df, cmd) = it.split_once(':')?;
                    let ma = parse_ma(cmd).into_iter().next()?;
                    Some((df.to_string(), json!(ma)))
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "cgameDf": gb.get("cgame_df").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>()).unwrap_or_default(),
        "dynamicFps": str_at(dfg.as_ref().and_then(|d| d.get("dynamic_fps"))),
        "dynamicFpsM": str_at(dfg.as_ref().and_then(|d| d.get("dynamic_fps_M"))),
        "sgamePid120": pid120,
        "sgameDfMa": df_ma,
        "foregroundMa": fg_ma,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(fps: JoyFpsMode, cpu: JoyCpuMode) -> JoyoseConfig {
        JoyoseConfig { enabled: true, fps_mode: fps, fps_offset: 4.0, cpu_mode: cpu, cpu_floor: vec![2265600, 2707200, 2707200, 2745600] }
    }

    fn sample() -> String {
        json!({
            "game_booster": {
                "cgame_df": ["com.tencent.tmgp.sgame@120#10:0,45:110,46:90"],
                "dynamic_fps_global": { "dynamic_fps": "10:0,43:90,45:60", "dynamic_fps_M": "10:0,38:90,42.8:60" },
                "booster_config": {
                    "scene_config": [{
                        "scene_id": 10001,
                        "booster": [{ "permission": "root", "cmd": "glk#WA#1;MA#1440000,2131200,2131200,1939200;MF#0:307200" }],
                        "end": [{ "permission": "root", "cmd": "glk#MA#2265600,3148800,2956800,3052800" }]
                    }],
                    "ovrride_config": [{
                        "game_name": "SGAME",
                        "PID_T": "90#10:0 0 0 0 0 0,44.5:45 90 60 15 1.5 3;120#10:0 0 0 0 0 0,44.5:45 120 90 20 2 3",
                        "execute_cmd_by_df_T": "0:glk#MA#2265600,3148800,2956800,3052800$1:glk#MA#1248000,1497600,1497600,1248000$110:glk#MA#1248000,2438400,2188800,1824000",
                        "execute_cmd_by_temp_T": "60@42:glk#MA#1344000,1920000,1920000,2169600$44:glk#MA#1344000,1920000,1708800,2035200",
                        "dynamic_targetfps": "120#10:0,44:90;60#10:0",
                        "glk_max_change_temp": 40
                    }]
                }
            }
        })
        .to_string()
    }

    #[test]
    fn relax_and_raise() {
        let out: Value = serde_json::from_str(&tune(&sample(), &cfg(JoyFpsMode::Relax, JoyCpuMode::Raise)).unwrap()).unwrap();
        let gb = &out["game_booster"];
        assert_eq!(gb["cgame_df"][0], "com.tencent.tmgp.sgame@120#10:0,49:110,50:90");
        assert_eq!(gb["dynamic_fps_global"]["dynamic_fps"], "10:0,47:90,49:60");
        assert_eq!(gb["dynamic_fps_global"]["dynamic_fps_M"], "10:0,42:90,46.8:60");
        let sg = &gb["booster_config"]["ovrride_config"][0];
        assert_eq!(sg["PID_T"], "90#10:0 0 0 0 0 0,48.5:49 90 60 15 1.5 3;120#10:0 0 0 0 0 0,48.5:49 120 90 20 2 3");
        assert_eq!(sg["dynamic_targetfps"], "120#10:0,48:90;60#10:0");
        assert_eq!(sg["glk_max_change_temp"], 44.0);
        // df=1 保持，其他档位抬高到下限
        assert_eq!(
            sg["execute_cmd_by_df_T"],
            "0:glk#MA#2265600,3148800,2956800,3052800$1:glk#MA#1248000,1497600,1497600,1248000$110:glk#MA#2265600,2707200,2707200,2745600"
        );
        assert_eq!(sg["execute_cmd_by_temp_T"], "60@46:glk#MA#2265600,2707200,2707200,2745600$48:glk#MA#2265600,2707200,2707200,2745600");
        let scene = &gb["booster_config"]["scene_config"][0];
        assert_eq!(scene["booster"][0]["cmd"], "glk#WA#1;MA#2265600,2707200,2707200,2745600;MF#0:307200");
        // 退出恢复命令不变
        assert_eq!(scene["end"][0]["cmd"], "glk#MA#2265600,3148800,2956800,3052800");
    }

    #[test]
    fn off_and_max() {
        let out: Value = serde_json::from_str(&tune(&sample(), &cfg(JoyFpsMode::Off, JoyCpuMode::Max)).unwrap()).unwrap();
        let gb = &out["game_booster"];
        assert_eq!(gb["cgame_df"][0], "com.tencent.tmgp.sgame@120#10:0,45:0,46:0");
        assert_eq!(gb["dynamic_fps_global"]["dynamic_fps"], "10:0,43:0,45:0");
        let sg = &gb["booster_config"]["ovrride_config"][0];
        assert!(sg.get("PID_T").is_none());
        assert_eq!(gb["booster_config"]["scene_config"][0]["booster"][0]["cmd"], "glk#WA#1;MA#2265600,3148800,2956800,3052800;MF#0:307200");
    }

    #[test]
    fn stock_is_identity() {
        let c = cfg(JoyFpsMode::Stock, JoyCpuMode::Stock);
        assert_eq!(tune(&sample(), &c).unwrap(), canonical(&sample()));
    }

    #[test]
    fn summary_reads_keys() {
        let s = summary(&sample());
        assert_eq!(s["foregroundMa"], json!([1440000, 2131200, 2131200, 1939200]));
        assert_eq!(s["sgamePid120"], "120#10:0 0 0 0 0 0,44.5:45 120 90 20 2 3");
        assert_eq!(s["sgameDfMa"]["110"], json!([1248000, 2438400, 2188800, 1824000]));
    }
}
