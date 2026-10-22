//! 读取温度、频率、电池、屏幕与前台应用等系统状态

use crate::paths;
use serde::Serialize;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::process::Command;

pub fn read(path: &str) -> String {
    fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

pub fn read_i64(path: &str) -> Option<i64> {
    read(path).parse().ok()
}

pub fn getprop(key: &str) -> String {
    Command::new("getprop").arg(key).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

pub fn board_temp() -> f64 {
    read_i64(&format!("{}/board_sensor_temp", paths::TM)).unwrap_or(0) as f64 / 1000.0
}

pub fn sconfig() -> i32 {
    read(paths::SCONFIG).parse().unwrap_or(0)
}

pub fn screen_on() -> bool {
    read(&format!("{}/screen_state", paths::TM)) != "0"
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub level: i64,
    pub current_ma: i64,
    pub temp: f64,
    pub status: String,
    pub charging: bool,
}

pub fn battery() -> Battery {
    let b = "/sys/class/power_supply/battery";
    let status = read(&format!("{b}/status"));
    let usb = read_i64("/sys/class/power_supply/usb/online").unwrap_or(0) == 1;
    let wls = read_i64("/sys/class/power_supply/wireless/online").unwrap_or(0) == 1;
    Battery {
        level: read_i64(&format!("{b}/capacity")).unwrap_or(0),
        current_ma: read_i64(&format!("{b}/current_now")).unwrap_or(0) / 1000,
        temp: read_i64(&format!("{b}/temp")).unwrap_or(0) as f64 / 10.0,
        charging: status == "Charging" || status == "Full" || usb || wls,
        status,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempItem {
    pub id: String,
    pub label: &'static str,
    pub value: f64,
}

const NAMED: &[(&str, &str)] = &[
    ("cpu_therm", "SoC 附近"),
    ("battery", "电池"),
    ("quiet_therm", "机身"),
    ("charger_therm0", "充电 IC"),
    ("pa_therm1", "射频"),
    ("wifi_therm", "Wi-Fi"),
    ("display_therm", "屏幕"),
    ("wireless_therm", "无线充电"),
    ("flash_therm", "闪光灯"),
];

pub fn temps() -> Vec<TempItem> {
    let mut cpu: f64 = 0.0;
    let mut gpu: f64 = 0.0;
    let mut named: Vec<TempItem> = Vec::new();
    if let Ok(rd) = fs::read_dir("/sys/class/thermal") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("thermal_zone") {
                continue;
            }
            let dir = e.path();
            let ty = read(&dir.join("type").to_string_lossy());
            let is_cpu = ty.starts_with("cpu-") && ty.matches('-').count() == 3;
            let is_gpu = ty.starts_with("gpuss-");
            let label = NAMED.iter().find(|(n, _)| *n == ty).map(|(_, l)| *l);
            if !is_cpu && !is_gpu && label.is_none() {
                continue;
            }
            let Some(t) = read_i64(&dir.join("temp").to_string_lossy()) else { continue };
            let t = t as f64 / 1000.0;
            if is_cpu {
                cpu = cpu.max(t);
            } else if is_gpu {
                gpu = gpu.max(t);
            } else if let Some(label) = label {
                named.push(TempItem { id: ty, label, value: t });
            }
        }
    }
    named.sort_by_key(|t| NAMED.iter().position(|(n, _)| *n == t.id));
    let mut out = vec![TempItem { id: "cpu".into(), label: "CPU 核心", value: cpu }, TempItem { id: "gpu".into(), label: "GPU", value: gpu }];
    out.extend(named);
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    pub policy: String,
    pub cpus: String,
    pub first: u32,
    pub cur: u64,
    pub max: u64,
    pub hw: u64,
    pub freqs: Vec<u64>,
}

pub fn clusters() -> Vec<Cluster> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir("/sys/devices/system/cpu/cpufreq") else { return out };
    let mut dirs: Vec<_> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("policy")).collect();
    dirs.sort_by_key(|n| n[6..].parse::<u32>().unwrap_or(0));
    for p in dirs {
        let d = format!("/sys/devices/system/cpu/cpufreq/{p}");
        let cpus = read(&format!("{d}/related_cpus"));
        out.push(Cluster {
            first: cpus.split_whitespace().next().and_then(|c| c.parse().ok()).unwrap_or(0),
            cpus,
            cur: read_i64(&format!("{d}/scaling_cur_freq")).unwrap_or(0) as u64,
            max: read_i64(&format!("{d}/scaling_max_freq")).unwrap_or(0) as u64,
            hw: read_i64(&format!("{d}/cpuinfo_max_freq")).unwrap_or(0) as u64,
            freqs: read(&format!("{d}/scaling_available_frequencies")).split_whitespace().filter_map(|f| f.parse().ok()).collect(),
            policy: p,
        });
    }
    out
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    pub cur: u64,
    pub max: u64,
    pub hw: u64,
    pub level: i64,
    pub freqs: Vec<u64>,
}

pub fn gpu() -> Gpu {
    let k = "/sys/class/kgsl/kgsl-3d0";
    let mut freqs: Vec<u64> = read(&format!("{k}/gpu_available_frequencies")).split_whitespace().filter_map(|f| f.parse().ok()).collect();
    freqs.sort_unstable_by(|a, b| b.cmp(a));
    Gpu {
        cur: read_i64(&format!("{k}/gpuclk")).unwrap_or(0) as u64,
        max: read_i64(&format!("{k}/max_gpuclk")).unwrap_or(0) as u64,
        hw: freqs.first().copied().unwrap_or(0),
        level: read_i64(&format!("{k}/thermal_pwrlevel")).unwrap_or(0),
        freqs,
    }
}

/// top-app cpuset 中的应用进程包名（排除子进程与系统 uid）
pub fn foreground_packages() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for path in ["/dev/cpuset/top-app/cgroup.procs", "/dev/cpuset/top-app/tasks"] {
        let procs = read(path);
        if procs.is_empty() {
            continue;
        }
        for pid in procs.lines() {
            let Ok(meta) = fs::metadata(format!("/proc/{pid}")) else { continue };
            if meta.uid() % 100_000 < 10_000 {
                continue;
            }
            let Ok(cmd) = fs::read(format!("/proc/{pid}/cmdline")) else { continue };
            let name = String::from_utf8_lossy(cmd.split(|b| *b == 0).next().unwrap_or_default()).into_owned();
            if name.is_empty() || name.contains(':') || name.starts_with('/') || out.contains(&name) {
                continue;
            }
            out.push(name);
        }
        break;
    }
    // 桌面常驻 top-app，存在其他前台应用时将其排除
    if out.len() > 1 {
        out.retain(|p| !LAUNCHERS.contains(&p.as_str()));
    }
    out
}

const LAUNCHERS: &[&str] = &["com.miui.home", "com.mi.android.globallauncher", "com.android.launcher3", "com.google.android.apps.nexuslauncher"];

/// 屏幕支持的刷新率（Hz，降序），取自 DisplayManager 的模式列表
pub fn refresh_rates() -> Vec<u32> {
    let out = Command::new("dumpsys").arg("display").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let Some(line) = out.lines().find(|l| l.contains("supportedModes [")) else { return Vec::new() };
    let mut rates: Vec<u32> = line
        .split("fps=")
        .skip(1)
        .filter_map(|s| s.split(|c: char| c != '.' && !c.is_ascii_digit()).next()?.parse::<f64>().ok())
        .map(|f| f.round() as u32)
        .collect();
    rates.sort_unstable_by(|a, b| b.cmp(a));
    rates.dedup();
    rates
}
