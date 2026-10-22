mod conf;
mod crypto;
mod daemon;
mod install;
mod paths;
mod profile;
mod stock;
mod system;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use profile::{Config, STOCK_ID, build_all, build_scene};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use stock::Stock;

/// 写入模块日志（超过 256 KiB 时截断旧内容）
#[macro_export]
macro_rules! logln {
    ($($arg:tt)*) => { $crate::log_line(&format!($($arg)*)) };
}

pub fn log_line(msg: &str) {
    if fs::metadata(paths::LOG).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let old = fs::read_to_string(paths::LOG).unwrap_or_default();
        let _ = fs::write(paths::LOG, &old[old.len() / 2..].split_once('\n').map(|x| x.1).unwrap_or(""));
    }
    let ts = std::process::Command::new("date").arg("+%m-%d %H:%M:%S").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(paths::LOG) {
        let _ = writeln!(f, "{ts} {msg}");
    }
}

#[derive(Parser)]
#[command(name = "thermalctl", version, about = "小米温控调度 · 后端")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 实时状态（JSON）
    Status,
    /// 人类可读的状态摘要（模块「执行」按钮）
    Summary,
    /// 设备信息、配置、内置方案与频率表（JSON）
    Info,
    /// 官方场景配置的结构化数据（JSON）
    Scenes,
    /// 保存配置（从文件读取 JSON）并立即生效
    Save { file: String },
    /// 预览方案生成结果（JSON）
    Preview {
        /// 方案 ID；省略时使用配置中的默认方案
        #[arg(long)]
        profile: Option<String>,
        /// 方案 JSON 文件（用于未保存的编辑）
        #[arg(long)]
        file: Option<String>,
        /// 只输出某个场景的完整文本
        #[arg(long)]
        scene: Option<String>,
    },
    /// 立即重新应用当前配置
    Apply,
    /// 关闭总开关并恢复官方温控
    Reset,
    /// 最近的温控动作（JSON）
    Log {
        #[arg(short, default_value_t = 150)]
        n: usize,
    },
    /// mi_thermald 当前加载的配置
    Loaded,
    /// 解密任意 mi_thermald 配置文件
    Decrypt { file: String },
    /// 加密明文为 mi_thermald 配置
    Encrypt { input: String, output: String },
    /// 运行守护进程
    Daemon,
}

fn print(v: Value) {
    println!("{v}");
}

fn wait_generation(before: u64) -> bool {
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(150));
        if daemon::State::load().generation > before {
            return true;
        }
    }
    false
}

/// 让守护进程重新应用；守护进程未运行时直接应用默认方案
fn apply_now() -> Result<Value> {
    let before = daemon::State::load().generation;
    if daemon::notify() {
        if !wait_generation(before) {
            bail!("守护进程未响应");
        }
        let st = daemon::State::load();
        if !st.error.is_empty() {
            bail!("{}", st.error);
        }
        return Ok(serde_json::to_value(st)?);
    }
    let cfg = Config::load();
    let stock = Stock::load()?;
    let id = if cfg.enabled && !daemon::module_disabled() { cfg.active.clone() } else { STOCK_ID.into() };
    let p = cfg.find(&id).context("方案不存在")?;
    let _l = install::lock()?;
    let o = install::sync(&build_all(&p, &stock), cfg.immutable)?;
    Ok(json!({ "effective": p.id, "effectiveName": p.name, "installed": o.count, "daemon": false }))
}

fn parse_dump(line: &str) -> Option<Value> {
    // 10-22 15:31:41[MONITOR-BAT][VIRTUAL-SENSOR0 39949] {[battery 1000][...
    let (time, rest) = line.split_at(line.find('[')?);
    let mut parts = rest.splitn(4, ']');
    let rule = parts.next()?.trim_start_matches('[');
    let sensor = parts.next()?.trim_start_matches('[');
    let action = parts.next()?.trim_start_matches(" {[");
    let (sname, sval) = sensor.rsplit_once(' ')?;
    let (dev, val) = action.rsplit_once(' ')?;
    let raw: i64 = sval.parse().ok()?;
    let reading = if sname == "BAT_SOC" { format!("电量 {raw}%") } else { format!("{:.1}°C", raw as f64 / 1000.0) };
    Some(json!({ "time": time.trim(), "rule": rule, "sensor": sname, "reading": reading, "device": dev, "value": val }))
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Status => {
            let cfg = Config::load();
            let st = daemon::State::load();
            let sc = system::sconfig();
            let scene = Stock::load().map(|s| s.scene_for(sc)).unwrap_or_else(|_| "thermal-normal.conf".into());
            print(json!({
                "board": system::board_temp(),
                "sconfig": sc,
                "scene": scene,
                "sceneLabel": stock::scene_label(&scene),
                "temps": system::temps(),
                "clusters": system::clusters(),
                "gpu": system::gpu(),
                "battery": system::battery(),
                "screenOn": system::screen_on(),
                "enabled": cfg.enabled,
                "active": cfg.active,
                "daemon": { "running": daemon::running_pid().is_some(), "state": st },
                "installed": install::installed().len(),
                "moduleDisabled": daemon::module_disabled(),
            }));
        }
        Cmd::Summary => {
            let cfg = Config::load();
            let st = daemon::State::load();
            let running = daemon::running_pid().is_some();
            println!("小米温控调度 v{}", env!("CARGO_PKG_VERSION"));
            println!("────────────────────────────");
            println!("温控传感器   {:.1} °C", system::board_temp());
            let sc = system::sconfig();
            let scene = Stock::load().map(|s| s.scene_for(sc)).unwrap_or_default();
            println!("当前场景     {}（sconfig {sc}）", stock::scene_label(&scene));
            println!("配置密钥     {}", if crypto::active_key() == 0 { "旧版（thermalopenssl.h）" } else { "新版" });
            println!("守护进程     {}", if running { format!("运行中（pid {}）", st.pid) } else { "未运行".into() });
            println!("总开关       {}", if cfg.enabled { "已启用" } else { "已关闭" });
            if cfg.enabled && running {
                println!("生效方案     {}（{}）", st.effective_name, st.reason);
                println!("接管场景     {} 个", st.installed);
            }
            println!("过热保护     {}", if cfg.guard.enabled { format!("{:.0}°C 触发 / {:.0}°C 恢复", cfg.guard.trip, cfg.guard.release) } else { "已关闭".into() });
            if !st.error.is_empty() {
                println!("最近错误     {}", st.error);
            }
            println!("────────────────────────────");
            for c in system::clusters() {
                println!("{:<8}  上限 {:>4} / {:>4} MHz", c.policy, c.max / 1000, c.hw / 1000);
            }
            let g = system::gpu();
            println!("{:<8}  上限 {:>4} / {:>4} MHz", "gpu", g.max / 1_000_000, g.hw / 1_000_000);
            println!();
            println!("详细设置请打开 WebUI。");
        }
        Cmd::Info => {
            let cfg = Config::load();
            print(json!({
                "device": {
                    "model": system::getprop("ro.product.marketname"),
                    "codename": system::getprop("ro.product.device"),
                    "soc": system::getprop("ro.soc.model"),
                    "os": system::getprop("ro.mi.os.version.name"),
                    "build": system::getprop("ro.build.version.incremental"),
                    "android": system::getprop("ro.build.version.release"),
                },
                "version": env!("CARGO_PKG_VERSION"),
                "cipherKey": crypto::active_key(),
                "config": cfg,
                "builtins": profile::builtins(),
                "categories": conf::CATEGORIES.iter().map(|(id, name, desc)| json!({ "id": id, "name": name, "desc": desc })).collect::<Vec<_>>(),
                "clusters": system::clusters(),
                "gpu": system::gpu(),
            }));
        }
        Cmd::Scenes => {
            let stock = Stock::load()?;
            let scenes: Vec<Value> = stock
                .scenes
                .keys()
                .map(|f| {
                    let secs: Vec<Value> = stock
                        .sections(f)
                        .iter()
                        .map(|s| json!({ "name": s.name, "base": stock.base_name(&s.name), "category": conf::category(s), "kv": s.kv }))
                        .collect();
                    json!({ "file": f, "label": stock::scene_label(f), "text": stock.scenes[f], "sections": secs })
                })
                .collect();
            let map: Vec<Value> = stock.map.iter().map(|(k, v)| json!({ "sconfig": k, "file": v })).collect();
            print(json!({ "scenes": scenes, "map": map }));
        }
        Cmd::Save { file } => {
            let cfg = profile::read_config_file(&file)?;
            {
                let _l = install::lock()?;
                cfg.save()?;
            }
            let _ = fs::remove_file(&file);
            logln!("配置已保存（默认方案 {}，总开关 {}）", cfg.active, if cfg.enabled { "开" } else { "关" });
            // 配置已经落盘；应用失败只作为警告返回，避免界面误以为未保存
            match apply_now() {
                Ok(r) => print(json!({ "ok": true, "result": r })),
                Err(e) => print(json!({ "ok": true, "warning": format!("{e:#}") })),
            }
        }
        Cmd::Preview { profile: pid, file, scene } => {
            let cfg = Config::load();
            let stock = Stock::load()?;
            let p = match (file, pid) {
                (Some(f), _) => serde_json::from_str(&fs::read_to_string(&f)?).context("方案格式错误")?,
                (None, Some(id)) => cfg.find(&id).context("方案不存在")?,
                (None, None) => cfg.find(&cfg.active).context("方案不存在")?,
            };
            if let Some(f) = scene {
                let text = build_scene(&p, &stock, &f);
                print(json!({ "file": f, "changed": text.is_some(), "text": text.unwrap_or_else(|| stock.scenes.get(&f).cloned().unwrap_or_default()) }));
            } else {
                let all = build_all(&p, &stock);
                let list: Vec<Value> = stock
                    .scenes
                    .keys()
                    .map(|f| {
                        let before = stock.sections(f).len();
                        let after = all.get(f).map(|t| conf::parse(t).len()).unwrap_or(before);
                        json!({ "file": f, "label": stock::scene_label(f), "changed": all.contains_key(f), "before": before, "after": after })
                    })
                    .collect();
                print(json!({ "profile": p.id, "changed": all.len(), "scenes": list }));
            }
        }
        Cmd::Apply => print(json!({ "ok": true, "result": apply_now()? })),
        Cmd::Reset => {
            let mut cfg = Config::load();
            cfg.enabled = false;
            {
                let _l = install::lock()?;
                cfg.save()?;
            }
            logln!("已关闭总开关，恢复官方温控");
            print(json!({ "ok": true, "result": apply_now()? }));
        }
        Cmd::Log { n } => {
            let text = fs::read(paths::DUMP).unwrap_or_default();
            let tail = &text[text.len().saturating_sub(n * 600)..];
            let s = String::from_utf8_lossy(tail);
            let items: Vec<Value> = s.lines().rev().filter_map(parse_dump).take(n).collect();
            let module = system::read(paths::LOG);
            let mlog: Vec<&str> = module.lines().rev().take(80).collect();
            print(json!({ "thermal": items, "module": mlog }));
        }
        Cmd::Loaded => {
            let sc = system::sconfig();
            let scene = Stock::load().map(|s| s.scene_for(sc)).unwrap_or_default();
            print(json!({ "sconfig": sc, "scene": scene, "label": stock::scene_label(&scene), "text": system::read(paths::DECRYPT_TXT) }));
        }
        Cmd::Decrypt { file } => print!("{}", crypto::decrypt_text(&fs::read(&file)?)?),
        Cmd::Encrypt { input, output } => fs::write(output, crypto::encrypt(&fs::read(input)?))?,
        Cmd::Daemon => daemon::run()?,
    }
    Ok(())
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        println!("{}", json!({ "ok": false, "error": format!("{e:#}") }));
        std::process::exit(1);
    }
}
