//! 方案（profile）、自动化规则与配置文件，以及由方案生成场景配置的流水线

use crate::conf::{self, Section};
use crate::paths;
use crate::stock::Stock;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;

pub const STOCK_ID: &str = "stock";

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Keep,
    Offset,
    Disable,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CatRule {
    pub mode: Mode,
    /// 阈值偏移，单位 °C，可为负数（更早降频）
    pub offset: f64,
}

/// 单条规则的精确覆盖。key 形如 "thermal-normal.conf#SS-CPU7"，"*#SS-CPU7" 表示所有场景
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct RuleOverride {
    pub disabled: bool,
    pub trig: Option<Vec<String>>,
    pub clr: Option<Vec<String>>,
    pub target: Option<Vec<String>>,
    pub polling: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SceneMode {
    /// 跟随方案
    #[default]
    Profile,
    /// 此场景保持官方配置
    Stock,
    /// 使用自定义全文
    Text,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SceneOverride {
    pub mode: SceneMode,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub desc: String,
    /// 图标名，供 WebUI 展示
    pub icon: String,
    #[serde(skip_deserializing)]
    pub builtin: bool,
    /// 风险提示等级：0 安全，1 注意，2 高风险
    pub risk: u8,
    /// 所有场景统一使用此场景的策略
    pub lock: String,
    pub categories: BTreeMap<String, CatRule>,
    /// CPU 降频下限，key 为 cpu0 / cpu2 / cpu5 / cpu7 等，值为 kHz
    pub cpu_floor: BTreeMap<String, u64>,
    /// GPU 降频等级上限（数值越大频率越低），None 表示不限制
    pub gpu_max_level: Option<u32>,
    pub rules: BTreeMap<String, RuleOverride>,
    pub scenes: BTreeMap<String, SceneOverride>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TriggerKind {
    #[default]
    App,
    Charging,
    ScreenOff,
    BatteryBelow,
    TempAbove,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AutoRule {
    pub id: String,
    pub enabled: bool,
    pub kind: TriggerKind,
    pub packages: Vec<String>,
    pub value: f64,
    pub profile: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Guard {
    pub enabled: bool,
    /// 主板温度达到此值（°C）时临时切回官方温控
    pub trip: f64,
    /// 降到此值以下后恢复
    pub release: f64,
}

impl Default for Guard {
    fn default() -> Self {
        Guard { enabled: true, trip: 48.0, release: 43.0 }
    }
}

/// 应用刷新率规则：列表中的应用位于前台时，将系统刷新率上限锁定为 rate
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct RefreshRule {
    pub id: String,
    pub enabled: bool,
    pub packages: Vec<String>,
    pub rate: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct RefreshConfig {
    pub enabled: bool,
    /// 过热保护触发时暂停锁定，交还系统控制
    pub respect_guard: bool,
    pub rules: Vec<RefreshRule>,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        RefreshConfig { enabled: true, respect_guard: true, rules: Vec::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub version: u32,
    /// 总开关；关闭时不接管任何场景
    pub enabled: bool,
    /// 默认方案
    pub active: String,
    pub profiles: Vec<Profile>,
    pub automation: bool,
    pub rules: Vec<AutoRule>,
    pub guard: Guard,
    /// 对覆盖文件设置 chattr +i
    pub immutable: bool,
    /// 守护进程轮询间隔（毫秒）
    pub interval: u64,
    pub refresh: RefreshConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: 2,
            enabled: false,
            active: "balanced".into(),
            profiles: Vec::new(),
            automation: true,
            rules: Vec::new(),
            guard: Guard::default(),
            immutable: true,
            interval: 2000,
            refresh: RefreshConfig::default(),
        }
    }
}

fn cats(list: &[(&str, Mode, f64)]) -> BTreeMap<String, CatRule> {
    list.iter().map(|(id, mode, offset)| (id.to_string(), CatRule { mode: *mode, offset: *offset })).collect()
}

fn preset(id: &str, name: &str, icon: &str, risk: u8, desc: &str, c: &[(&str, Mode, f64)]) -> Profile {
    Profile {
        id: id.into(),
        name: name.into(),
        desc: desc.into(),
        icon: icon.into(),
        builtin: true,
        risk,
        categories: cats(c),
        ..Default::default()
    }
}

pub fn builtins() -> Vec<Profile> {
    use Mode::*;
    let mut gaming = preset(
        "gaming",
        "游戏",
        "gamepad",
        1,
        "CPU / GPU 阈值 +8°C，并为大核设置 1.5 GHz 降频下限，减少团战掉帧。建议配合「按应用」自动切换使用。",
        &[("cpu", Offset, 8.0), ("gpu", Offset, 8.0), ("npu", Offset, 8.0), ("display", Offset, 6.0), ("net", Offset, 5.0)],
    );
    gaming.cpu_floor = [("cpu2", 1_497_600), ("cpu5", 1_497_600), ("cpu7", 1_478_400)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
    gaming.gpu_max_level = Some(1);
    vec![
        preset(STOCK_ID, "官方", "verified", 0, "完全使用系统自带的温控策略，不做任何修改。", &[]),
        preset(
            "cool",
            "清凉",
            "ac_unit",
            0,
            "CPU / GPU 提前 3°C 降频，机身更凉、续航更长，适合日常与夏季。",
            &[("cpu", Offset, -3.0), ("gpu", Offset, -3.0), ("npu", Offset, -3.0)],
        ),
        preset("light", "轻度", "eco", 0, "CPU / GPU 降频阈值整体提高 3°C，几乎不影响手感温度。", &[("cpu", Offset, 3.0), ("gpu", Offset, 3.0), ("npu", Offset, 3.0)]),
        preset(
            "balanced",
            "均衡",
            "balance",
            0,
            "CPU / GPU 阈值 +6°C、亮度限制 +3°C，在流畅与温度之间取得平衡。",
            &[("cpu", Offset, 6.0), ("gpu", Offset, 6.0), ("npu", Offset, 6.0), ("display", Offset, 3.0)],
        ),
        preset(
            "performance",
            "性能",
            "speed",
            1,
            "CPU / GPU 阈值 +10°C，亮度与网络限制同步放宽，适合长时间高负载。",
            &[("cpu", Offset, 10.0), ("gpu", Offset, 10.0), ("npu", Offset, 10.0), ("display", Offset, 5.0), ("net", Offset, 5.0)],
        ),
        gaming,
        preset(
            "charging",
            "快充",
            "bolt",
            1,
            "充电限流阈值 +4°C，其余保持官方。建议设为「充电时」自动切换。",
            &[("charge", Offset, 4.0)],
        ),
        preset(
            "extreme",
            "极限",
            "local_fire_department",
            2,
            "移除 CPU / GPU / NPU 降频与亮度限制，仅保留充电、高温保护与低电量保护。请务必开启过热保护。",
            &[("cpu", Disable, 0.0), ("gpu", Disable, 0.0), ("npu", Disable, 0.0), ("display", Disable, 0.0), ("net", Offset, 8.0)],
        ),
    ]
}

impl Config {
    pub fn load() -> Config {
        let Ok(text) = fs::read_to_string(paths::CONFIG) else { return Config::default() };
        match serde_json::from_str::<Config>(&text) {
            Ok(c) if c.version >= 2 => c,
            _ => migrate_v1(&text).unwrap_or_default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let tmp = format!("{}.tmp", paths::CONFIG);
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&tmp, paths::CONFIG)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for p in &self.profiles {
            if p.id.is_empty() || p.id == STOCK_ID || builtins().iter().any(|b| b.id == p.id) {
                bail!("方案 ID「{}」无效或与内置方案冲突", p.id);
            }
            if !seen.insert(&p.id) {
                bail!("方案 ID「{}」重复", p.id);
            }
        }
        if self.find(&self.active).is_none() {
            bail!("默认方案「{}」不存在", self.active);
        }
        for r in self.rules.iter().filter(|r| r.enabled) {
            if self.find(&r.profile).is_none() {
                bail!("自动化规则引用了不存在的方案「{}」", r.profile);
            }
        }
        for r in self.refresh.rules.iter().filter(|r| r.enabled) {
            if !(24..=240).contains(&r.rate) {
                bail!("刷新率 {} Hz 超出有效范围", r.rate);
            }
            if r.packages.is_empty() {
                bail!("应用刷新率规则未选择应用");
            }
        }
        if self.guard.release >= self.guard.trip {
            bail!("过热保护的恢复温度必须低于触发温度");
        }
        Ok(())
    }

    pub fn all_profiles(&self) -> Vec<Profile> {
        let mut v = builtins();
        v.extend(self.profiles.iter().cloned());
        v
    }

    pub fn find(&self, id: &str) -> Option<Profile> {
        self.all_profiles().into_iter().find(|p| p.id == id)
    }
}

/// v1 配置（单一策略 + 手动覆盖）迁移为一个自定义方案
fn migrate_v1(text: &str) -> Option<Config> {
    #[derive(Deserialize)]
    struct V1Rule {
        mode: String,
        offset: f64,
    }
    #[derive(Deserialize)]
    struct V1 {
        enabled: bool,
        rules: BTreeMap<String, V1Rule>,
        #[serde(default)]
        lock: String,
        #[serde(default)]
        immutable: bool,
        #[serde(default)]
        manual: BTreeMap<String, String>,
    }
    let v1: V1 = serde_json::from_str(text).ok()?;
    let mut p = Profile { id: "custom".into(), name: "我的方案".into(), desc: "由旧版本设置迁移".into(), icon: "tune".into(), lock: v1.lock, ..Default::default() };
    for (k, r) in v1.rules {
        let mode = match r.mode.as_str() {
            "offset" => Mode::Offset,
            "disable" => Mode::Disable,
            _ => Mode::Keep,
        };
        p.categories.insert(k, CatRule { mode, offset: r.offset });
    }
    for (f, text) in v1.manual {
        p.scenes.insert(f, SceneOverride { mode: SceneMode::Text, text });
    }
    Some(Config { enabled: v1.enabled, active: "custom".into(), profiles: vec![p], immutable: v1.immutable, ..Config::default() })
}

fn shift(values: &[String], delta: i64) -> Vec<String> {
    values.iter().map(|v| if conf::is_temp(v) { (v.parse::<i64>().unwrap() + delta).to_string() } else { v.clone() }).collect()
}

fn apply_category(sec: &Section, rule: &CatRule) -> Option<Section> {
    match rule.mode {
        Mode::Keep => Some(sec.clone()),
        Mode::Disable => None,
        Mode::Offset => {
            let delta = (rule.offset * 1000.0).round() as i64;
            let sic = sec.algo() == "sic";
            let mut out = sec.clone();
            for (k, v) in out.kv.iter_mut() {
                if k == "trig" || k == "clr" || (sic && k == "target") {
                    *v = shift(v, delta);
                }
            }
            Some(out)
        }
    }
}

/// 降频下限：把 target 中低于下限的 CPU 频率抬高到下限
fn apply_cpu_floor(sec: &mut Section, floors: &BTreeMap<String, u64>) {
    let devs: Vec<String> = sec.devices().iter().map(|s| s.to_string()).collect();
    let Some(targets) = sec.get("target").cloned() else { return };
    let new: Vec<String> = targets
        .iter()
        .map(|t| {
            t.split('+')
                .enumerate()
                .map(|(i, part)| match (devs.get(i).and_then(|d| floors.get(d)), part.parse::<u64>()) {
                    (Some(&floor), Ok(v)) if floor > 0 && v < floor => floor.to_string(),
                    _ => part.to_string(),
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect();
    sec.set("target", new);
}

fn apply_gpu_cap(sec: &mut Section, max: u32) {
    if let Some(targets) = sec.get("target").cloned() {
        let new = targets
            .iter()
            .map(|t| match t.parse::<u32>() {
                Ok(v) if v > max => max.to_string(),
                _ => t.clone(),
            })
            .collect();
        sec.set("target", new);
    }
}

fn apply_override(sec: &mut Section, o: &RuleOverride) {
    for (key, val) in [("trig", &o.trig), ("clr", &o.clr), ("target", &o.target)] {
        if let Some(v) = val {
            sec.set(key, v.clone());
        }
    }
    if let Some(p) = &o.polling {
        sec.set("polling", vec![p.clone()]);
    }
}

/// 生成场景的最终配置文本；None 表示与官方一致，无需覆盖
pub fn build_scene(profile: &Profile, stock: &Stock, file: &str) -> Option<String> {
    if profile.id == STOCK_ID {
        return None;
    }
    let own = stock.sections(file);
    if let Some(sc) = profile.scenes.get(file) {
        match sc.mode {
            SceneMode::Stock => return None,
            SceneMode::Text => return (!sc.text.trim().is_empty() && sc.text != stock.scenes[file]).then(|| sc.text.clone()),
            SceneMode::Profile => {}
        }
    }
    let mut secs = own.clone();
    if !profile.lock.is_empty() && profile.lock != file && stock.scenes.contains_key(&profile.lock) {
        secs = own.iter().filter(|s| !conf::is_policy(s)).cloned().collect();
        secs.extend(stock.sections(&profile.lock).into_iter().filter(conf::is_policy));
    }
    let mut out = Vec::with_capacity(secs.len());
    for sec in secs {
        let cat = conf::category(&sec);
        if cat == "sensor" || cat == "bcl" {
            out.push(sec);
            continue;
        }
        let Some(mut sec) = (match profile.categories.get(cat) {
            Some(rule) => apply_category(&sec, rule),
            None => Some(sec),
        }) else {
            continue;
        };
        if cat == "cpu" && !profile.cpu_floor.is_empty() {
            apply_cpu_floor(&mut sec, &profile.cpu_floor);
        }
        if cat == "gpu" {
            if let Some(max) = profile.gpu_max_level {
                apply_gpu_cap(&mut sec, max);
            }
        }
        let base = stock.base_name(&sec.name);
        let o = profile.rules.get(&format!("{file}#{base}")).or_else(|| profile.rules.get(&format!("*#{base}")));
        if let Some(o) = o {
            if o.disabled {
                continue;
            }
            apply_override(&mut sec, o);
        }
        out.push(sec);
    }
    let text = conf::serialize(&out);
    (text != conf::serialize(&own)).then_some(text)
}

pub fn build_all(profile: &Profile, stock: &Stock) -> BTreeMap<String, String> {
    stock.scenes.keys().filter_map(|f| build_scene(profile, stock, f).map(|t| (f.clone(), t))).collect()
}

pub fn read_config_file(path: &str) -> Result<Config> {
    let text = fs::read_to_string(path).with_context(|| format!("读取 {path} 失败"))?;
    let cfg: Config = serde_json::from_str(&text).context("配置格式错误")?;
    cfg.validate()?;
    Ok(cfg)
}
