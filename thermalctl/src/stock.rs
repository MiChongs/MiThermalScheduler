//! 读取并解密系统自带的场景配置与场景映射表

use crate::{conf, crypto};
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::fs;

const DIRS: &[&str] = &["/odm/etc", "/vendor/etc"];

pub struct Stock {
    /// sconfig 数值 -> 场景文件名
    pub map: BTreeMap<i32, String>,
    /// 场景文件名 -> 解密后的官方配置
    pub scenes: BTreeMap<String, String>,
    prefixes: Vec<String>,
}

fn is_scene_file(name: &str) -> bool {
    name.starts_with("thermal-")
        && name.ends_with(".conf")
        && !name.ends_with("map.conf")
        && !["demo-ind", "chg-only", "boost"].iter().any(|x| name.contains(x))
}

pub fn scene_id(file: &str) -> &str {
    file.trim_start_matches("thermal-").trim_end_matches(".conf")
}

pub fn scene_label(file: &str) -> String {
    let mut id = scene_id(file);
    let mut tags = Vec::new();
    for (pre, tag) in [("iec-", "IEC"), ("per-", "性能模式"), ("hp-", "高性能")] {
        if let Some(rest) = id.strip_prefix(pre) {
            id = rest;
            tags.push(tag);
        }
    }
    if let Some(rest) = id.strip_suffix("-unfold") {
        id = rest;
        tags.push("展开");
    }
    let base = match id {
        "normal" => "日常",
        "huanji" => "换机",
        "abnormal" => "异常",
        "nightvideo" => "夜景视频",
        "dolbyvision" => "杜比视界",
        "phone" => "通话",
        "nolimits" => "无限制",
        "class0" => "重载应用",
        "youtube" => "YouTube",
        "arvr" => "AR / VR",
        "navigation" => "导航",
        "video" => "视频播放",
        "demo" => "演示",
        "videochat" => "视频通话",
        "camera" => "相机",
        "4k" => "4K 录像",
        "tgame" => "游戏（T）",
        "mgame" => "游戏（M）",
        "cgame" => "游戏（C）",
        "sphone" => "通话（S）",
        "yuanshen" => "原神",
        "xingtie" => "星穹铁道",
        "highfps" => "高帧率",
        "cclassvideo" => "视频（C）",
        other => other,
    };
    if tags.is_empty() { base.to_string() } else { format!("{base}（{}）", tags.join("，")) }
}

fn read_encrypted(name: &str) -> Option<String> {
    for dir in DIRS {
        if let Ok(bytes) = fs::read(format!("{dir}/{name}")) {
            return crypto::decrypt_text(&bytes).ok();
        }
    }
    None
}

impl Stock {
    pub fn load() -> Result<Stock> {
        let mut names: Vec<String> = Vec::new();
        for dir in DIRS {
            if let Ok(rd) = fs::read_dir(dir) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if is_scene_file(&n) && !names.contains(&n) {
                        names.push(n);
                    }
                }
            }
        }
        let mut scenes = BTreeMap::new();
        for n in names {
            if let Some(text) = read_encrypted(&n) {
                if text.trim_start().starts_with('[') {
                    scenes.insert(n, text);
                }
            }
        }
        if !scenes.contains_key("thermal-normal.conf") {
            bail!("未找到可解密的 thermal-normal.conf，设备不受支持");
        }
        let mut map = BTreeMap::new();
        let map_text = read_encrypted("thermal-odm-map.conf").or_else(|| read_encrypted("thermal-map.conf")).unwrap_or_default();
        for line in map_text.lines() {
            let l = line.trim().trim_start_matches('[').trim_end_matches(']');
            if let Some((n, f)) = l.split_once(':') {
                if let Ok(n) = n.trim().parse() {
                    map.insert(n, f.trim().to_string());
                }
            }
        }
        Ok(Stock::from_parts(map, scenes))
    }

    pub fn from_parts(map: BTreeMap<i32, String>, scenes: BTreeMap<String, String>) -> Stock {
        let mut prefixes: Vec<String> = scenes.keys().filter(|f| *f != "thermal-normal.conf").map(|f| scene_id(f).to_uppercase()).collect();
        prefixes.sort_by_key(|p| std::cmp::Reverse(p.len()));
        Stock { map, scenes, prefixes }
    }

    /// 去掉段名中的场景前缀，例如 TGAME-SS-CPU7 -> SS-CPU7
    pub fn base_name(&self, name: &str) -> String {
        let upper = name.to_uppercase();
        for p in &self.prefixes {
            if upper.len() > p.len() + 1 && upper.starts_with(p.as_str()) && upper.as_bytes()[p.len()] == b'-' {
                return name[p.len() + 1..].to_string();
            }
        }
        name.to_string()
    }

    pub fn scene_for(&self, sconfig: i32) -> String {
        self.map
            .get(&sconfig)
            .filter(|f| self.scenes.contains_key(*f))
            .cloned()
            .unwrap_or_else(|| "thermal-normal.conf".to_string())
    }

    pub fn sections(&self, file: &str) -> Vec<conf::Section> {
        self.scenes.get(file).map(|t| conf::parse(t)).unwrap_or_default()
    }
}
