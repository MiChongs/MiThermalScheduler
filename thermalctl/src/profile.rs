//! 方案（profile）、自动化规则与配置文件，以及由方案生成场景配置的流水线

use crate::conf::{self, Section};
use crate::paths;
use crate::stock::Stock;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;

pub const STOCK_ID: &str = "stock";
/// v3：过热保护默认值调整为 50 / 46°C；v4：规则分类拆分，内置方案重做
const CONFIG_VERSION: u32 = 4;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Keep,
    /// 按偏移与起控温度调整触发温度
    Offset,
    /// 移除该类规则
    Disable,
    /// 仅保留首档：保留最轻的一档限制，移除后续随温度升高的降频档位
    First,
    /// 推迟：合并为一档，在 value 指定的温度触发，目标取首档
    Defer,
    /// 替换为内置曲线（充电类规则）
    Curve,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CatRule {
    pub mode: Mode,
    /// 阈值偏移，单位 °C，可为负数（更早降频）
    pub offset: f64,
    /// 起控温度（°C）：首档触发温度低于此值时，将整条曲线压缩到此温度与上限之间；0 表示不设置
    pub start: f64,
    /// 推迟模式的触发温度（°C）
    pub value: f64,
    /// 曲线模式使用的内置曲线
    pub curve: String,
}

impl CatRule {
    pub fn new(mode: Mode) -> CatRule {
        CatRule { mode, ..Default::default() }
    }
    pub fn offset(offset: f64, start: f64) -> CatRule {
        CatRule { mode: Mode::Offset, offset, start, ..Default::default() }
    }
    pub fn defer(value: f64) -> CatRule {
        CatRule { mode: Mode::Defer, value, ..Default::default() }
    }
    pub fn curve(name: &str) -> CatRule {
        CatRule { mode: Mode::Curve, curve: name.into(), ..Default::default() }
    }
}

/// 内置充电曲线：(名称, 触发温度, 解除温度, 目标值)，温度单位为毫摄氏度
pub const CURVES: &[(&str, &[u32], &[u32], &[u32])] = &[
    // 日常：37°C 起逐级限流，较官方（33°C 起）明显放宽
    (
        "standard",
        &[37000, 40000, 41000, 42000, 43000, 44000, 45000, 46000, 47000],
        &[35000, 39000, 40000, 41000, 42000, 43000, 44000, 45000, 46000],
        &[100, 201, 301, 402, 502, 703, 805, 1006, 1208],
    ),
    // 温和：起点相同，各档限流更多，发热更低
    (
        "gentle",
        &[37000, 40000, 41000, 42000, 43000, 44000, 45000, 46000, 47000],
        &[35000, 39000, 40000, 41000, 42000, 43000, 44000, 45000, 46000],
        &[201, 302, 403, 503, 604, 804, 1006, 1208, 1410],
    ),
    // 游戏：边玩边充时持续轻度限流，40°C 起逐级增加
    (
        "game",
        &[15000, 40000, 41000, 42000, 43000, 44000, 45000, 46000, 47000],
        &[10000, 39000, 40000, 41000, 42000, 43000, 44000, 45000, 46000],
        &[202, 404, 505, 606, 707, 808, 1010, 1212, 1414],
    ),
    // 无线充电
    (
        "wireless",
        &[37000, 40000, 41000, 42000, 43000, 44000, 45000, 46000],
        &[35000, 39000, 40000, 41000, 42000, 43000, 44000, 45000],
        &[201, 302, 403, 503, 604, 804, 1006, 1208],
    ),
];

pub fn curve(name: &str) -> Option<(&'static [u32], &'static [u32], &'static [u32])> {
    CURVES.iter().find(|c| c.0 == name).map(|c| (c.1, c.2, c.3))
}

/// 游戏与跑分场景：mi_thermald 为其单独提供配置，模块可为其设置独立策略
pub fn is_game_scene(file: &str) -> bool {
    let id = crate::stock::scene_id(file);
    ["game", "xingtie", "yuanshen", "highfps", "nolimits"].iter().any(|k| id.contains(k))
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
    /// 游戏与跑分场景的分类策略，未设置的分类沿用 categories
    pub game: BTreeMap<String, CatRule>,
    /// CPU 降频下限，key 为 cpu0 / cpu2 / cpu5 / cpu7 等，值为 kHz
    pub cpu_floor: BTreeMap<String, u64>,
    /// GPU 降频等级上限（数值越大频率越低），None 表示不限制
    pub gpu_max_level: Option<u32>,
    pub rules: BTreeMap<String, RuleOverride>,
    pub scenes: BTreeMap<String, SceneOverride>,
    /// 放宽上限（°C）：放宽后的触发温度不超过此值，取自过热保护触发温度，运行时设置
    #[serde(skip)]
    pub ceiling: Option<f64>,
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
        Guard { enabled: true, trip: 50.0, release: 46.0 }
    }
}

/// 应用配置：列表中的应用位于前台时切换到指定方案，并可锁定刷新率上限
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AppRule {
    pub id: String,
    pub enabled: bool,
    pub packages: Vec<String>,
    /// 前台时使用的方案，空字符串表示不切换方案
    pub profile: String,
    /// 刷新率上限（Hz），0 表示不锁定
    pub refresh: u32,
}

/// 旧版应用刷新率规则（v2.2），加载时迁移为应用配置
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
    /// 应用配置中的刷新率锁定总开关
    pub enabled: bool,
    /// 过热保护触发时暂停锁定，交还系统控制
    pub respect_guard: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<RefreshRule>,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        RefreshConfig { enabled: true, respect_guard: false, rules: Vec::new() }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum JoyFpsMode {
    /// 保持官方
    Stock,
    /// 温控限帧的温度门限整体上调
    #[default]
    Relax,
    /// 关闭温控限帧
    Off,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum JoyCpuMode {
    /// 保持官方
    Stock,
    /// 游戏场景 CPU 上限不低于设定值
    #[default]
    Raise,
    /// 游戏场景 CPU 上限设为满频
    Max,
}

/// 充电加速：充电时在当前方案之上放宽充电限流规则
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ChargeBoost {
    pub enabled: bool,
    /// 充电类规则的温度门限上调值（°C）
    pub offset: f64,
    /// 同时移除 SIC-BAT 充电电流 PID 控制
    pub remove_sic: bool,
}

impl Default for ChargeBoost {
    fn default() -> Self {
        ChargeBoost { enabled: true, offset: 4.0, remove_sic: false }
    }
}

impl ChargeBoost {
    /// 在方案之上叠加充电放宽；充电类规则已被移除的方案保持不变
    pub fn overlay(&self, p: &Profile) -> Profile {
        let mut q = p.clone();
        let boost = |r: &mut CatRule| match r.mode {
            Mode::Keep => *r = CatRule::offset(self.offset, 0.0),
            Mode::Offset | Mode::Curve => r.offset = r.offset.max(self.offset),
            _ => {}
        };
        boost(q.categories.entry("charge".into()).or_default());
        if let Some(r) = q.game.get_mut("charge") {
            boost(r);
        }
        if self.remove_sic {
            q.rules.insert("*#SIC-BAT".into(), RuleOverride { disabled: true, ..Default::default() });
        }
        q.id = format!("{}+charge", p.id);
        q.name = format!("{}（充电加速）", p.name);
        q
    }
}

/// Joyose（游戏加速服务）调教
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct JoyoseConfig {
    pub enabled: bool,
    pub fps_mode: JoyFpsMode,
    /// 温控限帧门限上调值（°C）
    pub fps_offset: f64,
    pub cpu_mode: JoyCpuMode,
    /// 各 CPU 簇的游戏频率上限下限（kHz），顺序与 Joyose 的 clusterList 一致
    pub cpu_floor: Vec<u64>,
    /// 限帧温度的放宽上限（°C），取自过热保护触发温度，运行时设置
    #[serde(skip)]
    pub ceiling: Option<f64>,
}

impl Default for JoyoseConfig {
    fn default() -> Self {
        JoyoseConfig {
            enabled: false,
            fps_mode: JoyFpsMode::Relax,
            fps_offset: 4.0,
            cpu_mode: JoyCpuMode::Raise,
            cpu_floor: vec![2_265_600, 2_707_200, 2_707_200, 2_745_600],
            ceiling: None,
        }
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
    /// 场景规则开关
    pub automation: bool,
    /// 场景规则（充电 / 熄屏 / 低电量 / 高温）
    pub rules: Vec<AutoRule>,
    /// 应用配置
    pub apps: Vec<AppRule>,
    pub guard: Guard,
    /// 对覆盖文件设置 chattr +i
    pub immutable: bool,
    /// 守护进程轮询间隔（毫秒）
    pub interval: u64,
    pub refresh: RefreshConfig,
    pub joyose: JoyoseConfig,
    pub charge: ChargeBoost,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CONFIG_VERSION,
            enabled: false,
            active: "balanced".into(),
            profiles: Vec::new(),
            automation: true,
            rules: Vec::new(),
            apps: Vec::new(),
            guard: Guard::default(),
            immutable: true,
            interval: 2000,
            refresh: RefreshConfig::default(),
            joyose: JoyoseConfig::default(),
            charge: ChargeBoost::default(),
        }
    }
}

fn preset(id: &str, name: &str, icon: &str, risk: u8, desc: &str, daily: &[(&str, CatRule)], game: &[(&str, CatRule)]) -> Profile {
    let map = |l: &[(&str, CatRule)]| l.iter().map(|(k, r)| (k.to_string(), r.clone())).collect();
    Profile {
        id: id.into(),
        name: name.into(),
        desc: desc.into(),
        icon: icon.into(),
        builtin: true,
        risk,
        categories: map(daily),
        game: map(game),
        ..Default::default()
    }
}

pub fn builtins() -> Vec<Profile> {
    use Mode::*;
    // 日常场景：CPU、GPU、NPU 仅保留首档轻度限制，移除随温度升高的逐级降频；
    // 游戏与跑分场景：CPU、GPU 推迟到 60°C（启用过热保护时不超过保护温度）。
    // 关核、亮度、网络、高温提示与其他限制移除；传感器定义与低电量保护始终保留。
    let common = |charge: &str, boost: CatRule| {
        vec![
            ("cpu", CatRule::new(First)),
            ("core", CatRule::new(Disable)),
            ("gpu", CatRule::new(First)),
            ("npu", CatRule::new(First)),
            ("charge", CatRule::curve(charge)),
            ("wireless", CatRule::curve("wireless")),
            ("display", CatRule::new(Disable)),
            ("net", CatRule::new(Disable)),
            ("warn", CatRule::new(Disable)),
            ("boost", boost),
            ("other", CatRule::new(Disable)),
        ]
    };
    let game = || vec![("cpu", CatRule::defer(60.0)), ("gpu", CatRule::defer(60.0)), ("charge", CatRule::curve("game")), ("boost", CatRule::defer(60.0))];
    let mut performance = common("standard", CatRule::defer(60.0));
    for (k, r) in performance.iter_mut() {
        if ["cpu", "gpu", "npu"].contains(k) {
            *r = CatRule::new(Disable);
        }
    }
    let all_off: Vec<(&str, CatRule)> = conf::CATEGORIES.iter().map(|(id, _, _)| (*id, CatRule::new(Disable))).collect();
    vec![
        preset(STOCK_ID, "官方", "verified", 0, "使用系统官方温控策略，不做任何修改。", &[], &[]),
        preset(
            "cool",
            "清凉",
            "ac_unit",
            0,
            "日常场景保留首档轻度限频并持续限制性能加速，充电限流较温和；游戏场景推迟降频。温度优先。",
            &common("gentle", CatRule::defer(15.0)),
            &game(),
        ),
        preset(
            "balanced",
            "均衡",
            "balance",
            0,
            "日常场景保留首档轻度限频，移除逐级降频；游戏场景 CPU 与 GPU 推迟到 60°C 降频。兼顾性能与温度。",
            &common("standard", CatRule::defer(60.0)),
            &game(),
        ),
        preset(
            "performance",
            "性能",
            "speed",
            1,
            "移除全部场景的 CPU、GPU 与 NPU 限频，保留充电限流曲线。发热明显增加，请启用过热保护。",
            &performance,
            &[("charge", CatRule::curve("game"))],
        ),
        preset(
            "extreme",
            "极限",
            "local_fire_department",
            2,
            "移除除传感器定义与低电量保护以外的全部规则，包括充电限流与高温提示。仅建议短时测试使用，并必须启用过热保护。",
            &all_off,
            &[],
        ),
    ]
}

impl Config {
    pub fn load() -> Config {
        let Ok(text) = fs::read_to_string(paths::CONFIG) else { return Config::default() };
        let mut cfg = match serde_json::from_str::<Config>(&text) {
            Ok(c) if c.version >= 2 => c,
            _ => migrate_v1(&text).unwrap_or_default(),
        };
        if cfg.normalize() {
            // 旧格式迁移后立即写回，避免每次加载重复迁移
            let _ = cfg.save();
        }
        cfg
    }

    /// 将 v2.2 及以前的「按应用」场景规则与应用刷新率规则合并为应用配置
    pub fn normalize(&mut self) -> bool {
        fn merge(apps: &mut Vec<AppRule>, id: &str, enabled: bool, packages: &[String], profile: Option<&str>, refresh: Option<u32>) {
            let mut key: Vec<&String> = packages.iter().collect();
            key.sort();
            let found = apps.iter_mut().find(|a| {
                let mut k: Vec<&String> = a.packages.iter().collect();
                k.sort();
                k == key
            });
            let app = match found {
                Some(a) => a,
                None => {
                    apps.push(AppRule { id: id.to_string(), enabled, packages: packages.to_vec(), ..Default::default() });
                    apps.last_mut().unwrap()
                }
            };
            if let Some(p) = profile {
                app.profile = p.to_string();
            }
            if let Some(r) = refresh {
                app.refresh = r;
            }
            app.enabled |= enabled;
        }
        let mut changed = false;
        if self.version < CONFIG_VERSION {
            // 旧默认值 48 / 43°C 低于方案的放宽范围，升级为新默认值；用户自定义的值保留
            if self.version < 3 && self.guard.trip == 48.0 && self.guard.release == 43.0 {
                self.guard = Guard { enabled: self.guard.enabled, ..Guard::default() };
            }
            if self.version < 4 {
                // 已移除的内置方案改用均衡；均衡已在游戏场景推迟降频，无需单独的游戏方案
                let removed = |id: &mut String| {
                    if ["light", "gaming", "charging"].contains(&id.as_str()) {
                        *id = "balanced".into();
                    }
                };
                removed(&mut self.active);
                self.rules.iter_mut().for_each(|r| removed(&mut r.profile));
                self.apps.iter_mut().for_each(|a| removed(&mut a.profile));
                // 分类拆分：关核、无线充电、加速限制沿用原所属分类的设置
                for p in &mut self.profiles {
                    for (from, to) in [("cpu", "core"), ("charge", "wireless"), ("other", "boost")] {
                        if let Some(r) = p.categories.get(from).cloned() {
                            p.categories.entry(to.into()).or_insert(r);
                        }
                    }
                }
                // 过热保护期间暂停刷新率锁定会使游戏回落到 60 Hz，默认改为不暂停
                self.refresh.respect_guard = false;
            }
            self.version = CONFIG_VERSION;
            changed = true;
        }
        let legacy_app: Vec<AutoRule> = self.rules.iter().filter(|r| r.kind == TriggerKind::App).cloned().collect();
        let legacy_refresh = std::mem::take(&mut self.refresh.rules);
        if legacy_app.is_empty() && legacy_refresh.is_empty() {
            return changed;
        }
        self.rules.retain(|r| r.kind != TriggerKind::App);
        for r in &legacy_app {
            merge(&mut self.apps, &r.id, r.enabled, &r.packages, Some(&r.profile), None);
        }
        for r in &legacy_refresh {
            merge(&mut self.apps, &r.id, r.enabled, &r.packages, None, Some(r.rate));
        }
        true
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
                bail!("场景规则引用了不存在的方案「{}」", r.profile);
            }
        }
        for a in &self.apps {
            if a.packages.is_empty() {
                bail!("应用配置未选择应用");
            }
            if !a.profile.is_empty() && self.find(&a.profile).is_none() {
                bail!("应用配置引用了不存在的方案「{}」", a.profile);
            }
            if a.refresh != 0 && !(24..=240).contains(&a.refresh) {
                bail!("刷新率 {} Hz 超出有效范围", a.refresh);
            }
        }
        if !(0.0..=10.0).contains(&self.charge.offset) {
            bail!("充电加速上调值需在 0–10°C 之间");
        }
        if !(0.0..=15.0).contains(&self.joyose.fps_offset) {
            bail!("Joyose 温控限帧上调值需在 0–15°C 之间");
        }
        if self.guard.release >= self.guard.trip {
            bail!("过热保护的恢复温度必须低于触发温度");
        }
        if self.guard.enabled {
            if let Some(r) = self.rules.iter().find(|r| r.enabled && r.kind == TriggerKind::TempAbove && r.value >= self.guard.trip) {
                bail!("「高温时」场景规则的温度（{}°C）需低于过热保护触发温度（{}°C）", r.value, self.guard.trip);
            }
        }
        for p in &self.profiles {
            if p.categories.values().any(|c| c.start != 0.0 && !(30.0..=60.0).contains(&c.start)) {
                bail!("方案「{}」的起控温度需在 30–60°C 之间", p.name);
            }
        }
        Ok(())
    }

    /// 放宽上限：启用过热保护时，方案放宽后的触发温度不超过保护触发温度，
    /// 避免放宽区间落在保护区间内（该区间实际由官方温控接管）
    pub fn ceiling(&self) -> Option<f64> {
        self.guard.enabled.then_some(self.guard.trip)
    }

    pub fn all_profiles(&self) -> Vec<Profile> {
        let mut v = builtins();
        v.extend(self.profiles.iter().cloned());
        let ceiling = self.ceiling();
        for p in &mut v {
            p.ceiling = ceiling;
        }
        v
    }

    /// 实际使用的 Joyose 调教参数（含放宽上限）
    pub fn joyose_effective(&self) -> JoyoseConfig {
        JoyoseConfig { ceiling: self.ceiling(), ..self.joyose.clone() }
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
        p.categories.insert(k, CatRule { mode, offset: r.offset, ..Default::default() });
    }
    for (f, text) in v1.manual {
        p.scenes.insert(f, SceneOverride { mode: SceneMode::Text, text });
    }
    Some(Config { enabled: v1.enabled, active: "custom".into(), profiles: vec![p], immutable: v1.immutable, ..Config::default() })
}

/// 相邻档位压缩后的最小间隔（毫摄氏度）
const MIN_STEP: f64 = 500.0;

/// 温度映射：由一条规则的触发温度（毫摄氏度）、偏移、起控温度与放宽上限求出线性映射。
/// 曲线首档不低于起控温度，末档推迟 offset，且（不低于官方末档的前提下）不超过放宽上限
struct TempMap {
    lo: f64,
    hi: f64,
    new_lo: f64,
    new_hi: f64,
}

impl TempMap {
    fn new(trig: &[f64], rule: &CatRule, ceiling: Option<f64>) -> Option<TempMap> {
        let lo = trig.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = trig.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        if !lo.is_finite() {
            return None;
        }
        let gap = MIN_STEP * (trig.len().saturating_sub(1)) as f64;
        let off = rule.offset * 1000.0;
        let (mut new_lo, mut new_hi) = (lo + off, hi + off);
        let start = rule.start * 1000.0;
        if rule.start > 0.0 && new_lo < start {
            new_lo = start;
            new_hi = new_hi.max(start + gap);
        }
        if let Some(c) = ceiling {
            let cap = (c * 1000.0).max(hi);
            if new_hi > cap {
                new_hi = cap;
                new_lo = new_lo.min((cap - gap).max(lo));
            }
        }
        Some(TempMap { lo, hi, new_lo, new_hi })
    }

    fn map(&self, x: f64) -> f64 {
        let y = if self.hi > self.lo { self.new_lo + (x - self.lo) * (self.new_hi - self.new_lo) / (self.hi - self.lo) } else { x + (self.new_lo - self.lo) };
        (y / 100.0).round() * 100.0
    }
}

fn temps(values: &[String]) -> Vec<f64> {
    values.iter().filter(|v| conf::is_temp(v)).filter_map(|v| v.parse::<f64>().ok()).collect()
}

fn fmt_temp(v: f64) -> String {
    format!("{}", v as i64)
}

/// 推迟模式的实际触发温度（毫摄氏度）：不超过放宽上限，且不低于官方末档
fn defer_temp(sec: &Section, value: f64, ceiling: Option<f64>) -> f64 {
    let hi = temps(&sec.get("trig").cloned().unwrap_or_default()).into_iter().fold(0.0, f64::max);
    let v = value * 1000.0;
    match ceiling {
        Some(c) if v > c * 1000.0 => (c * 1000.0).max(hi).min(v),
        _ => v,
    }
}

/// 生成内置充电曲线规则；offset 与放宽上限作用于曲线的触发温度
fn curve_section(name: &str, rule: &CatRule, ceiling: Option<f64>) -> Option<Section> {
    let (trig, clr, target) = curve(&rule.curve)?;
    let t: Vec<f64> = trig.iter().map(|v| *v as f64).collect();
    let m = TempMap::new(&t, &CatRule { start: 0.0, ..rule.clone() }, ceiling)?;
    let s = |v: &[u32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let shift = |v: &[u32]| v.iter().zip(trig).map(|(c, tr)| if *tr >= 10_000 { fmt_temp(m.map(*tr as f64) - (*tr as f64 - *c as f64)) } else { c.to_string() }).collect();
    Some(Section {
        name: name.into(),
        kv: vec![
            ("algo_type".into(), vec!["monitor".into()]),
            ("sensor".into(), vec!["VIRTUAL-SENSOR0".into()]),
            ("device".into(), vec!["battery".into()]),
            ("polling".into(), vec!["2000".into()]),
            ("trig".into(), trig.iter().map(|tr| if *tr >= 10_000 { fmt_temp(m.map(*tr as f64)) } else { tr.to_string() }).collect()),
            ("clr".into(), shift(clr)),
            ("target".into(), s(target)),
        ],
    })
}

/// template：日常场景 CPU 规则「仅保留首档」时使用的日常配置同名规则，各场景统一采用日常配置的首档
fn apply_category(sec: &Section, rule: &CatRule, ceiling: Option<f64>, template: Option<&Section>) -> Option<Section> {
    match rule.mode {
        Mode::Keep => Some(sec.clone()),
        Mode::Disable => None,
        Mode::First => {
            let src = template.unwrap_or(sec);
            let mut out = sec.clone();
            for key in ["trig", "clr", "target"] {
                if let Some(v) = src.get(key).and_then(|v| v.first()) {
                    out.set(key, vec![v.clone()]);
                }
            }
            Some(out)
        }
        Mode::Defer => {
            let t = defer_temp(sec, rule.value, ceiling);
            let clr = if t <= 20_000.0 { 10_000.0 } else { t - 3000.0 };
            let mut out = sec.clone();
            out.set("trig", vec![fmt_temp(t)]);
            out.set("clr", vec![fmt_temp(clr)]);
            if let Some(v) = sec.get("target").and_then(|v| v.first()) {
                out.set("target", vec![v.clone()]);
            }
            Some(out)
        }
        // 有线充电曲线在场景末尾统一追加，原有规则移除；无线充电替换各规则的档位
        Mode::Curve => {
            if conf::category(sec) != "wireless" {
                return None;
            }
            let c = curve_section(&sec.name, rule, ceiling)?;
            let mut out = sec.clone();
            for key in ["trig", "clr", "target"] {
                out.set(key, c.get(key).cloned().unwrap_or_default());
            }
            Some(out)
        }
        Mode::Offset => {
            let sic = sec.algo() == "sic";
            let trig = sec.get("trig").cloned().unwrap_or_default();
            // SIC 规则的 target 也是温度，且首档用于开启控制，不适用起控温度
            let r = if sic { CatRule { start: 0.0, ..rule.clone() } } else { rule.clone() };
            let Some(m) = TempMap::new(&temps(&trig), &r, ceiling) else { return Some(sec.clone()) };
            let fmt = |v: f64| format!("{}", v as i64);
            let mut out = sec.clone();
            for (k, v) in out.kv.iter_mut() {
                match k.as_str() {
                    "trig" => *v = v.iter().map(|x| if conf::is_temp(x) { fmt(m.map(x.parse().unwrap())) } else { x.clone() }).collect(),
                    // 解除温度保持与触发温度的原有回差
                    "clr" => {
                        *v = v
                            .iter()
                            .enumerate()
                            .map(|(i, c)| match (trig.get(i).filter(|t| conf::is_temp(t)), conf::is_temp(c)) {
                                (Some(t), true) => {
                                    let t: f64 = t.parse().unwrap();
                                    fmt(m.map(t) - (t - c.parse::<f64>().unwrap()))
                                }
                                (None, true) => fmt(m.map(c.parse().unwrap())),
                                _ => c.clone(),
                            })
                            .collect()
                    }
                    "target" if sic => *v = v.iter().map(|x| if conf::is_temp(x) { fmt(m.map(x.parse().unwrap())) } else { x.clone() }).collect(),
                    _ => {}
                }
            }
            Some(out)
        }
    }
}

/// 内置充电曲线规则的段名
pub const CHARGE_CURVE_NAME: &str = "MTS-CHARGE-BAT";

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
    let game = is_game_scene(file);
    let rule_of = |cat: &str| if game { profile.game.get(cat).or_else(|| profile.categories.get(cat)) } else { profile.categories.get(cat) };
    let normal = if game { Vec::new() } else { stock.sections("thermal-normal.conf") };
    let mut out = Vec::with_capacity(secs.len());
    for sec in secs {
        let cat = conf::category(&sec);
        if cat == "sensor" || cat == "bcl" {
            out.push(sec);
            continue;
        }
        let base = stock.base_name(&sec.name);
        let template = (cat == "cpu" && base.starts_with("SS-CPU")).then(|| normal.iter().find(|n| stock.base_name(&n.name) == base)).flatten();
        let Some(mut sec) = (match rule_of(cat) {
            Some(rule) => apply_category(&sec, rule, profile.ceiling, template),
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
        let o = profile.rules.get(&format!("{file}#{base}")).or_else(|| profile.rules.get(&format!("*#{base}")));
        if let Some(o) = o {
            if o.disabled {
                continue;
            }
            apply_override(&mut sec, o);
        }
        out.push(sec);
    }
    if let Some(rule) = rule_of("charge").filter(|r| r.mode == Mode::Curve) {
        out.extend(curve_section(CHARGE_CURVE_NAME, rule, profile.ceiling));
    }
    let text = conf::serialize(&out);
    (text != conf::serialize(&own)).then_some(text)
}

pub fn build_all(profile: &Profile, stock: &Stock) -> BTreeMap<String, String> {
    stock.scenes.keys().filter_map(|f| build_scene(profile, stock, f).map(|t| (f.clone(), t))).collect()
}

pub fn read_config_file(path: &str) -> Result<Config> {
    let text = fs::read_to_string(path).with_context(|| format!("读取 {path} 失败"))?;
    let mut cfg: Config = serde_json::from_str(&text).context("配置格式错误")?;
    cfg.normalize();
    cfg.validate()?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_app_rules() {
        let mut c = Config::default();
        c.rules.push(AutoRule { id: "a".into(), enabled: true, kind: TriggerKind::App, packages: vec!["g.one".into()], value: 0.0, profile: "gaming".into() });
        c.rules.push(AutoRule { id: "b".into(), enabled: true, kind: TriggerKind::Charging, packages: vec![], value: 0.0, profile: "charging".into() });
        c.refresh.rules.push(RefreshRule { id: "r1".into(), enabled: true, packages: vec!["g.one".into()], rate: 120 });
        c.refresh.rules.push(RefreshRule { id: "r2".into(), enabled: true, packages: vec!["g.two".into()], rate: 90 });
        assert!(c.normalize());
        assert_eq!(c.rules.len(), 1);
        assert_eq!(c.rules[0].kind, TriggerKind::Charging);
        assert!(c.refresh.rules.is_empty());
        assert_eq!(c.apps.len(), 2);
        assert_eq!((c.apps[0].profile.as_str(), c.apps[0].refresh), ("gaming", 120));
        assert_eq!((c.apps[1].profile.as_str(), c.apps[1].refresh), ("", 90));
        // 再次规整不产生变化
        let snapshot = c.clone();
        assert!(!c.normalize());
        assert_eq!(c, snapshot);
    }

    #[test]
    fn charge_overlay() {
        let cb = ChargeBoost { enabled: true, offset: 4.0, remove_sic: true };
        let stock = builtins().into_iter().find(|p| p.id == STOCK_ID).unwrap();
        let o = cb.overlay(&stock);
        assert_eq!(o.id, "stock+charge");
        assert_eq!(o.categories["charge"], CatRule::offset(4.0, 0.0));
        assert!(o.rules["*#SIC-BAT"].disabled);
        // 曲线模式：叠加偏移，日常与游戏曲线均生效
        let bal = builtins().into_iter().find(|p| p.id == "balanced").unwrap();
        let o = cb.overlay(&bal);
        assert_eq!((o.categories["charge"].mode, o.categories["charge"].offset), (Mode::Curve, 4.0));
        assert_eq!(o.game["charge"].offset, 4.0);
        // 方案自身更大的偏移保留
        let mut p = stock.clone();
        p.categories.insert("charge".into(), CatRule::offset(6.0, 0.0));
        assert_eq!(cb.overlay(&p).categories["charge"].offset, 6.0);
        // 已移除充电规则的方案不变
        let mut d = stock.clone();
        d.categories.insert("charge".into(), CatRule::new(Mode::Disable));
        assert_eq!(cb.overlay(&d).categories["charge"].mode, Mode::Disable);
    }

    fn sec(trig: &str, clr: &str) -> Section {
        let v = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        Section {
            name: "SS-CPU2".into(),
            kv: vec![
                ("algo_type".into(), vec!["ss".into()]),
                ("device".into(), vec!["cpu2".into()]),
                ("trig".into(), v(trig)),
                ("clr".into(), v(clr)),
                ("target".into(), v("1 2 3 4")),
            ],
        }
    }

    #[test]
    fn start_and_ceiling() {
        let s = sec("25000 37000 42000 48000", "23000 34000 41000 47000");
        let rule = CatRule::offset(8.0, 42.0);
        // 起控温度 42°C，末档推迟到 56°C
        let o = apply_category(&s, &rule, None, None).unwrap();
        assert_eq!(o.get("trig").unwrap(), &vec!["42000", "49300", "52300", "56000"]);
        assert_eq!(o.get("clr").unwrap(), &vec!["40000", "46300", "51300", "55000"]);
        // 过热保护 50°C：末档不超过 50°C
        let o = apply_category(&s, &rule, Some(50.0), None).unwrap();
        assert_eq!(o.get("trig").unwrap().last().unwrap(), "50000");
        assert_eq!(o.get("trig").unwrap()[0], "42000");
        // 官方末档已高于上限时保持官方末档，不收紧
        let hot = sec("52000", "50000");
        let o = apply_category(&hot, &CatRule::offset(4.0, 0.0), Some(50.0), None).unwrap();
        assert_eq!(o.get("trig").unwrap(), &vec!["52000"]);
        // 负偏移不受起控温度与上限影响
        let o = apply_category(&s, &CatRule::offset(-2.0, 0.0), Some(50.0), None).unwrap();
        assert_eq!(o.get("trig").unwrap(), &vec!["23000", "35000", "40000", "46000"]);
    }

    #[test]
    fn first_defer_curve() {
        let s = sec("25000 37000 42000 48000", "23000 34000 41000 47000");
        // 仅保留首档
        let o = apply_category(&s, &CatRule::new(Mode::First), None, None).unwrap();
        assert_eq!((o.get("trig").unwrap().clone(), o.get("target").unwrap().clone()), (vec!["25000".to_string()], vec!["1".to_string()]));
        // 使用日常配置首档
        let tpl = sec("42000 45000", "40000 43000");
        let o = apply_category(&s, &CatRule::new(Mode::First), None, Some(&tpl)).unwrap();
        assert_eq!(o.get("trig").unwrap(), &vec!["42000"]);
        // 推迟到 60°C；过热保护 50°C 时不超过 50°C，但不低于官方末档
        let o = apply_category(&s, &CatRule::defer(60.0), None, None).unwrap();
        assert_eq!((o.get("trig").unwrap()[0].as_str(), o.get("clr").unwrap()[0].as_str()), ("60000", "57000"));
        let o = apply_category(&s, &CatRule::defer(60.0), Some(46.0), None).unwrap();
        assert_eq!(o.get("trig").unwrap()[0], "48000");
        let o = apply_category(&s, &CatRule::defer(15.0), Some(50.0), None).unwrap();
        assert_eq!((o.get("trig").unwrap()[0].as_str(), o.get("clr").unwrap()[0].as_str()), ("15000", "10000"));
        // 充电曲线
        let c = curve_section(CHARGE_CURVE_NAME, &CatRule::curve("standard"), None).unwrap();
        assert_eq!(c.get("trig").unwrap()[0], "37000");
        assert_eq!(c.get("target").unwrap()[0], "100");
        let mut boosted = CatRule::curve("standard");
        boosted.offset = 4.0;
        let c = curve_section(CHARGE_CURVE_NAME, &boosted, Some(50.0)).unwrap();
        assert_eq!(c.get("trig").unwrap().last().unwrap(), "50000");
        assert_eq!(c.get("clr").unwrap()[0], "39000");
    }
}
