//! 守护进程：按自动化规则选择方案、过热保护、覆盖文件自愈，并维护模块描述

use crate::profile::{Config, Profile, STOCK_ID, TriggerKind, build_all};
use crate::stock::Stock;
use crate::{install, joyose, logln, paths, refresh, system};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static RELOAD: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_usr1(_: libc::c_int) {
    RELOAD.store(true, Ordering::SeqCst);
}

extern "C" fn on_term(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    pub pid: i32,
    pub effective: String,
    pub effective_name: String,
    pub reason: String,
    pub rule_id: String,
    pub foreground: String,
    pub guard: bool,
    pub installed: usize,
    pub error: String,
    pub since: u64,
    pub generation: u64,
    /// 当前锁定刷新率的应用与刷新率（0 表示未锁定）
    pub refresh_pkg: String,
    pub refresh_rate: u32,
    /// Joyose 调教是否已注入
    pub joyose: bool,
    pub joyose_error: String,
}

impl State {
    pub fn load() -> State {
        fs::read_to_string(paths::STATE).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn save(&self) {
        let tmp = format!("{}.tmp", paths::STATE);
        if fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default()).is_ok() {
            let _ = fs::rename(&tmp, paths::STATE);
        }
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn running_pid() -> Option<i32> {
    let pid: i32 = system::read(paths::PID).parse().ok()?;
    let cmd = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    String::from_utf8_lossy(&cmd).contains("thermalctl").then_some(pid)
}

pub fn notify() -> bool {
    match running_pid() {
        Some(pid) => unsafe { libc::kill(pid, libc::SIGUSR1) == 0 },
        None => false,
    }
}

/// 在管理器中停用或标记卸载后立即恢复官方温控，不必等重启
pub fn module_disabled() -> bool {
    let dir = paths::MODULE_PROP.trim_end_matches("/module.prop");
    fs::metadata(format!("{dir}/disable")).is_ok() || fs::metadata(format!("{dir}/remove")).is_ok()
}

struct Inputs {
    temp: f64,
    screen_on: bool,
    battery: system::Battery,
    foreground: Vec<String>,
}

struct Decision {
    profile: Profile,
    reason: String,
    rule_id: String,
}

fn decide(cfg: &Config, inp: &Inputs, guard: &mut bool, active_rules: &mut Vec<String>) -> Decision {
    let pick = |id: &str| cfg.find(id).unwrap_or_else(|| cfg.find(STOCK_ID).unwrap());
    if module_disabled() {
        return Decision { profile: pick(STOCK_ID), reason: "模块已停用".into(), rule_id: String::new() };
    }
    if !cfg.enabled {
        return Decision { profile: pick(STOCK_ID), reason: "温控调度未启用".into(), rule_id: String::new() };
    }
    if cfg.guard.enabled {
        if inp.temp >= cfg.guard.trip {
            *guard = true;
        } else if inp.temp <= cfg.guard.release {
            *guard = false;
        }
        if *guard {
            return Decision { profile: pick(STOCK_ID), reason: format!("过热保护：主板 {:.1}°C", inp.temp), rule_id: String::new() };
        }
    } else {
        *guard = false;
    }
    // 应用配置优先于场景规则
    let app_hit = cfg.apps.iter().filter(|a| a.enabled && !a.profile.is_empty()).find_map(|a| inp.foreground.iter().find(|p| a.packages.contains(p)).map(|p| (a, p)));
    if let Some((app, pkg)) = app_hit {
        *active_rules = Vec::new();
        return Decision { profile: pick(&app.profile), reason: format!("前台应用 {pkg}"), rule_id: app.id.clone() };
    }
    let mut now_active = Vec::new();
    let mut found: Option<Decision> = None;
    if cfg.automation {
        for r in cfg.rules.iter().filter(|r| r.enabled) {
            let was = active_rules.contains(&r.id);
            let (hit, why) = match r.kind {
                TriggerKind::App => match inp.foreground.iter().find(|p| r.packages.contains(p)) {
                    Some(p) => (true, format!("前台应用 {p}")),
                    None => (false, String::new()),
                },
                TriggerKind::Charging => (inp.battery.charging, "正在充电".into()),
                TriggerKind::ScreenOff => (!inp.screen_on, "屏幕关闭".into()),
                TriggerKind::BatteryBelow => {
                    let limit = if was { r.value + 3.0 } else { r.value };
                    ((inp.battery.level as f64) < limit, format!("电量低于 {}%", r.value))
                }
                TriggerKind::TempAbove => {
                    let limit = if was { r.value - 2.0 } else { r.value };
                    (inp.temp >= limit, format!("主板温度 ≥ {}°C", r.value))
                }
            };
            if hit {
                now_active.push(r.id.clone());
                if found.is_none() {
                    found = Some(Decision { profile: pick(&r.profile), reason: why, rule_id: r.id.clone() });
                }
            }
        }
    }
    *active_rules = now_active;
    found.unwrap_or_else(|| Decision { profile: pick(&cfg.active), reason: "默认方案".into(), rule_id: String::new() })
}

pub fn describe(cfg: &Config, st: &State) -> String {
    if !st.error.is_empty() {
        return format!("【异常】{}。已恢复系统官方温控，详情请在 WebUI 中查看。", st.error);
    }
    if module_disabled() {
        return "【已停用】模块已停用，系统官方温控已恢复。".into();
    }
    if !cfg.enabled || st.effective.is_empty() || st.effective == STOCK_ID {
        return "【待机】当前使用系统官方温控。请在 WebUI 中选择温控方案并启用。".into();
    }
    let guard = if cfg.guard.enabled { format!("{:.0}°C", cfg.guard.trip) } else { "关闭".into() };
    let state = if st.guard { "过热保护中" } else { "运行中" };
    let mut extra = String::new();
    if st.refresh_rate > 0 {
        extra.push_str(&format!("；刷新率锁定 {} Hz", st.refresh_rate));
    }
    if st.joyose {
        extra.push_str("；Joyose 调教已启用");
    }
    format!(
        "【{state}】当前方案：{}（{}）；接管场景 {} 个；过热保护 {guard}；场景规则 {} 条，应用配置 {} 个{extra}。",
        st.effective_name,
        st.reason,
        st.installed,
        if cfg.automation { cfg.rules.iter().filter(|r| r.enabled).count() } else { 0 },
        cfg.apps.iter().filter(|a| a.enabled).count()
    )
}

pub fn set_description(text: &str) {
    let Ok(prop) = fs::read_to_string(paths::MODULE_PROP) else { return };
    let new: String = prop
        .lines()
        .map(|l| if l.starts_with("description=") { format!("description={text}") } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    if new != prop {
        let _ = fs::write(paths::MODULE_PROP, new);
    }
}

fn config_mtime() -> Option<SystemTime> {
    fs::metadata(paths::CONFIG).and_then(|m| m.modified()).ok()
}

pub fn run() -> Result<()> {
    fs::create_dir_all(paths::DATA)?;
    if let Some(pid) = running_pid() {
        if pid != std::process::id() as i32 {
            anyhow::bail!("守护进程已在运行（pid {pid}）");
        }
    }
    fs::write(paths::PID, std::process::id().to_string())?;
    unsafe {
        libc::signal(libc::SIGUSR1, on_usr1 as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as *const () as libc::sighandler_t);
    }
    let stock = Stock::load()?;
    logln!("守护进程启动，pid {}，场景 {} 个", std::process::id(), stock.scenes.len());

    let mut cfg = Config::load();
    let mut mtime = config_mtime();
    let mut st = State { pid: std::process::id() as i32, generation: State::load().generation, ..Default::default() };
    let mut guard = false;
    let mut active_rules: Vec<String> = Vec::new();
    let mut current: Option<(String, BTreeMap<String, String>)> = None;
    let mut tick: u64 = 0;
    let mut last_desc = String::new();
    let mut force = true;
    let mut saved = State::default();
    let mut rate_ctl = refresh::Controller::default();
    let mut joy_force = true;

    while !STOP.load(Ordering::SeqCst) {
        if RELOAD.swap(false, Ordering::SeqCst) || config_mtime() != mtime {
            cfg = Config::load();
            mtime = config_mtime();
            force = true;
            joy_force = true;
            logln!("配置已重新加载");
        }
        let needs_fg = cfg.apps.iter().any(|a| a.enabled && ((cfg.enabled && !a.profile.is_empty()) || (cfg.refresh.enabled && a.refresh > 0)));
        let inp = Inputs {
            temp: system::board_temp(),
            screen_on: system::screen_on(),
            battery: system::battery(),
            foreground: if needs_fg { system::foreground_packages() } else { Vec::new() },
        };
        let mut d = decide(&cfg, &inp, &mut guard, &mut active_rules);
        // 充电加速：充电时（总开关开启、未触发过热保护）在当前方案之上放宽充电限流
        if cfg.enabled && cfg.charge.enabled && inp.battery.charging && !guard && !module_disabled() {
            d.profile = cfg.charge.overlay(&d.profile);
            d.reason = format!("{}，充电中", d.reason);
        }
        let switching = current.as_ref().map(|(id, _)| id != &d.profile.id).unwrap_or(true);

        if switching || force {
            let _l = install::lock()?;
            let plain = build_all(&d.profile, &stock);
            match install::sync(&plain, cfg.immutable) {
                Ok(o) => {
                    if switching {
                        logln!("切换方案：{}（{}），覆盖 {} 个场景", d.profile.name, d.reason, o.count);
                        st.since = now();
                    }
                    st.error.clear();
                    st.installed = o.count;
                    current = Some((d.profile.id.clone(), plain));
                }
                Err(e) => {
                    logln!("应用失败：{e:#}");
                    st.error = format!("{e:#}");
                    st.installed = 0;
                    current = Some((STOCK_ID.into(), BTreeMap::new()));
                }
            }
            force = false;
            st.generation += 1;
        } else if tick % 15 == 0 {
            if let Some((_, plain)) = &current {
                if !plain.is_empty() && !install::intact(plain) {
                    logln!("检测到覆盖文件被外部修改，正在恢复");
                    let _l = install::lock()?;
                    let _ = install::sync(plain, cfg.immutable);
                    st.generation += 1;
                }
            }
        }

        // Joyose 调教：配置变化时立即同步，平时每 30 个周期复核一次（被云控同步覆盖时重新注入）
        if joy_force || tick % 30 == 0 {
            joy_force = false;
            let _l = install::lock()?;
            match joyose::sync(&cfg.joyose, module_disabled()) {
                Ok(_) => st.joyose_error.clear(),
                Err(e) => {
                    if st.joyose_error.is_empty() {
                        logln!("Joyose 调教失败：{e:#}");
                    }
                    st.joyose_error = format!("{e:#}");
                }
            }
            st.joyose = joyose::injected();
        }

        rate_ctl.update(refresh::Controller::desired(&cfg.refresh, &cfg.apps, &inp.foreground, guard, module_disabled()), tick);
        let (refresh_pkg, refresh_rate) = rate_ctl.active().cloned().unwrap_or_default();

        let new_state = State {
            effective: d.profile.id.clone(),
            effective_name: d.profile.name.clone(),
            reason: d.reason,
            rule_id: d.rule_id,
            foreground: inp.foreground.first().cloned().unwrap_or_default(),
            guard,
            refresh_pkg,
            refresh_rate,
            ..st.clone()
        };
        st = new_state;
        if st != saved || tick % 30 == 0 {
            st.save();
            saved = st.clone();
        }
        let desc = describe(&cfg, &st);
        if desc != last_desc {
            set_description(&desc);
            last_desc = desc;
        }

        tick += 1;
        let interval = cfg.interval.clamp(500, 10_000);
        let mut slept = 0;
        while slept < interval && !RELOAD.load(Ordering::SeqCst) && !STOP.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(100));
            slept += 100;
        }
    }
    rate_ctl.release();
    logln!("守护进程退出");
    let _ = fs::remove_file(paths::PID);
    Ok(())
}
