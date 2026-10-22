//! mi_thermald 配置文本的解析、序列化与规则分类

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Section {
    pub name: String,
    pub kv: Vec<(String, Vec<String>)>,
}

impl Section {
    pub fn get(&self, key: &str) -> Option<&Vec<String>> {
        self.kv.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn first(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.first()).map(String::as_str)
    }

    pub fn set(&mut self, key: &str, values: Vec<String>) {
        match self.kv.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = values,
            None => self.kv.push((key.to_string(), values)),
        }
    }

    pub fn algo(&self) -> String {
        self.first("algo_type").unwrap_or_default().to_ascii_lowercase()
    }

    pub fn devices(&self) -> Vec<&str> {
        self.first("device").map(|d| d.split('+').collect()).unwrap_or_default()
    }
}

pub fn parse(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            out.push(Section { name: line[1..line.len() - 1].to_string(), kv: Vec::new() });
        } else if let Some(sec) = out.last_mut() {
            let mut parts = line.split_whitespace();
            if let Some(key) = parts.next() {
                sec.kv.push((key.to_string(), parts.map(str::to_string).collect()));
            }
        }
    }
    out
}

pub fn serialize(secs: &[Section]) -> String {
    let mut s = String::new();
    for (i, sec) in secs.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push('[');
        s.push_str(&sec.name);
        s.push_str("]\n");
        for (k, v) in &sec.kv {
            s.push_str(k);
            for x in v {
                s.push('\t');
                s.push_str(x);
            }
            s.push('\n');
        }
    }
    s
}

/// 可调节的规则分类；sensor（传感器定义）与 bcl（低电量保护）不在其中，始终原样保留
pub const CATEGORIES: &[(&str, &str, &str)] = &[
    ("cpu", "CPU", "各簇降频、关核（hotplug）、LMH"),
    ("gpu", "GPU", "GPU 频率等级限制"),
    ("npu", "NPU", "AI 计算单元（CDSP）限制"),
    ("charge", "充电", "有线 / 无线充电电流限制"),
    ("display", "屏幕亮度", "高温时限制最高亮度"),
    ("net", "网络", "5G 回落、Wi-Fi 降速、并发下载"),
    ("warn", "高温保护", "过热提示、关闭闪光灯与相机"),
    ("other", "其他", "手电筒、通话、boost"),
];

fn device_category(d: &str) -> &'static str {
    if d.starts_with("cpu") || d.starts_with("hotplug_cpu") || d.starts_with("lmh_cpu") {
        "cpu"
    } else if d.starts_with("gpu") {
        "gpu"
    } else if d.starts_with("cdsp") {
        "npu"
    } else if matches!(d, "battery" | "thermal_fcc_override" | "wireless_charge" | "wls_limit") {
        "charge"
    } else if d.contains("brightness") || d.contains("backlight") {
        "display"
    } else if ["modem", "wifi", "market_download", "ntn", "poor_modem", "pa_"].iter().any(|p| d.starts_with(p)) {
        "net"
    } else if d.starts_with("temp_state") {
        "warn"
    } else {
        "other"
    }
}

pub fn category(sec: &Section) -> &'static str {
    let algo = sec.algo();
    let Some(dev) = sec.first("device") else { return "sensor" };
    if algo == "virtual" || algo == "simulated" || dev.starts_with("VIRTUAL-SENSOR") {
        return "sensor";
    }
    if sec.first("sensor") == Some("BAT_SOC") || sec.get("reverse").is_some() {
        return "bcl";
    }
    let devs = sec.devices();
    for (id, _, _) in CATEGORIES {
        if devs.iter().any(|d| device_category(d) == *id) {
            return id;
        }
    }
    "other"
}

pub fn is_policy(sec: &Section) -> bool {
    category(sec) != "sensor"
}

/// 温度值：毫摄氏度整数，且至少 10°C，避免误改非温度字段
pub fn is_temp(v: &str) -> bool {
    v.parse::<i64>().map(|n| n >= 10_000).unwrap_or(false)
}
