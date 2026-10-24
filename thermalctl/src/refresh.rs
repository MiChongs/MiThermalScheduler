//! 应用刷新率：指定应用位于前台时，锁定 HyperOS 的刷新率上限（secure miui_refresh_rate）
//!
//! 「电量与性能」会按场景改写该值（例如把游戏判定为视频场景后限制为 60 Hz）。
//! 锁定期间定期复核并重新写入；应用离开前台后恢复为用户在显示设置中选择的刷新率。
//! 不恢复锁定前读取的值：游戏启动时系统往往已将其改为 60，写回会使刷新率停留在 60 Hz。

use crate::logln;
use crate::profile::{AppRule, RefreshConfig};
use std::process::{Command, Stdio};

const KEY: &str = "miui_refresh_rate";
/// 锁定期间每隔多少个检测周期复核一次（settings 命令开销较大）
const RECHECK_TICKS: u64 = 3;

fn get() -> Option<String> {
    let out = Command::new("settings").args(["get", "secure", KEY]).stderr(Stdio::null()).output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty() && v != "null").then_some(v)
}

fn put(value: &str) {
    let _ = Command::new("settings").args(["put", "secure", KEY, value]).stderr(Stdio::null()).status();
}

fn delete() {
    let _ = Command::new("settings").args(["delete", "secure", KEY]).stderr(Stdio::null()).status();
}

/// 用户在显示设置中选择的刷新率
fn preferred() -> Option<u32> {
    for key in ["user_refresh_rate", "peak_refresh_rate"] {
        let out = Command::new("settings").args(["get", "system", key]).stderr(Stdio::null()).output().ok()?;
        let v = String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok();
        if let Some(v) = v.filter(|v| *v >= 30.0) {
            return Some(v.round() as u32);
        }
    }
    None
}

#[derive(Default)]
pub struct Controller {
    /// 当前锁定的 (包名, 刷新率)
    active: Option<(String, u32)>,
}

impl Controller {
    pub fn active(&self) -> Option<&(String, u32)> {
        self.active.as_ref()
    }

    /// 按配置与前台应用求出期望的锁定目标
    pub fn desired(cfg: &RefreshConfig, apps: &[AppRule], foreground: &[String], guard: bool, disabled: bool) -> Option<(String, u32)> {
        if !cfg.enabled || disabled || (guard && cfg.respect_guard) {
            return None;
        }
        apps.iter()
            .filter(|a| a.enabled && a.refresh > 0)
            .find_map(|a| foreground.iter().find(|p| a.packages.contains(p)).map(|p| (p.clone(), a.refresh)))
    }

    pub fn update(&mut self, desired: Option<(String, u32)>, tick: u64) {
        match (desired, &self.active) {
            (Some((pkg, rate)), None) => {
                let before = get();
                put(&rate.to_string());
                logln!("刷新率锁定为 {rate} Hz（{pkg}），锁定前 {}", before.as_deref().unwrap_or("未设置"));
                self.active = Some((pkg, rate));
            }
            (Some((pkg, rate)), Some((cur_pkg, cur_rate))) => {
                if rate != *cur_rate || pkg != *cur_pkg {
                    put(&rate.to_string());
                    logln!("刷新率锁定为 {rate} Hz（{pkg}）");
                    self.active = Some((pkg, rate));
                } else if tick % RECHECK_TICKS == 0 && get().as_deref() != Some(rate.to_string().as_str()) {
                    put(&rate.to_string());
                    logln!("刷新率被系统改写，已恢复为 {rate} Hz");
                }
            }
            (None, Some(_)) => self.release(),
            (None, None) => {}
        }
    }

    /// 解除锁定，恢复为用户选择的刷新率
    pub fn release(&mut self) {
        if self.active.take().is_none() {
            return;
        }
        match preferred() {
            Some(v) => {
                put(&v.to_string());
                logln!("刷新率锁定已解除，恢复为 {v} Hz");
            }
            None => {
                delete();
                logln!("刷新率锁定已解除");
            }
        }
    }
}
