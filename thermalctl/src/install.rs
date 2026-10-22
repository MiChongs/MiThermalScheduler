//! 把生成的配置加密写入 /data/vendor/thermal/config，并让 mi_thermald 重新加载

use crate::{crypto, logln, paths, system};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

const SELINUX_LABEL: &[u8] = b"u:object_r:thermal_data_file:s0\0";
const SYSTEM_GID: u32 = 1000;

/// 进程间互斥：守护进程与命令行不会同时写入
pub struct Lock(#[allow(dead_code)] File);

pub fn lock() -> Result<Lock> {
    fs::create_dir_all(paths::DATA)?;
    let f = File::options().create(true).truncate(false).write(true).open(paths::LOCK)?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
        bail!("无法获取文件锁");
    }
    Ok(Lock(f))
}

pub fn installed() -> Vec<String> {
    system::read(paths::INSTALLED).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

fn chattr(path: &str, flag: &str) {
    let _ = Command::new("chattr").arg(flag).arg(path).stderr(Stdio::null()).status();
}

fn label(path: &str) {
    if let Ok(p) = CString::new(path) {
        unsafe {
            libc::lsetxattr(p.as_ptr(), c"security.selinux".as_ptr(), SELINUX_LABEL.as_ptr().cast(), SELINUX_LABEL.len(), 0);
        }
    }
}

fn chown_system(path: &str) {
    if let Ok(p) = CString::new(path) {
        unsafe {
            libc::chown(p.as_ptr(), 0, SYSTEM_GID);
        }
    }
}

fn remove_one(name: &str) {
    let dst = format!("{}/{name}", paths::CFG_DIR);
    chattr(&dst, "-i");
    let _ = fs::remove_file(&dst);
    let bak = format!("{}/{name}", paths::BACKUP);
    if fs::copy(&bak, &dst).is_ok() {
        chown_system(&dst);
        let _ = fs::set_permissions(&dst, fs::Permissions::from_mode(0o644));
        label(&dst);
        let _ = fs::remove_file(&bak);
    }
}

fn write_one(name: &str, bytes: &[u8], ours: bool, immutable: bool) -> Result<()> {
    let dst = format!("{}/{name}", paths::CFG_DIR);
    chattr(&dst, "-i");
    if !ours && fs::metadata(&dst).is_ok() {
        fs::create_dir_all(paths::BACKUP)?;
        let bak = format!("{}/{name}", paths::BACKUP);
        if fs::metadata(&bak).is_err() {
            fs::copy(&dst, &bak)?;
        }
    }
    fs::write(&dst, bytes).with_context(|| format!("写入 {dst} 失败"))?;
    chown_system(&dst);
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o644))?;
    label(&dst);
    if immutable {
        chattr(&dst, "+i");
    }
    Ok(())
}

/// 切换一次 sconfig 再切回，使 mi_thermald 重新读取当前场景
pub fn reload() {
    let cur = system::read(paths::SCONFIG);
    if cur.is_empty() {
        return;
    }
    let alt = if cur == "0" { "7" } else { "0" };
    let _ = fs::write(paths::SCONFIG, alt);
    sleep(Duration::from_millis(800));
    let _ = fs::write(paths::SCONFIG, &cur);
    sleep(Duration::from_millis(900));
}

/// mi_thermald 当前加载的配置能否被正确解析
pub fn verify() -> bool {
    system::read(paths::DECRYPT_TXT).starts_with('[')
}

#[derive(Default)]
pub struct Outcome {
    pub count: usize,
}

/// 使 /data/vendor/thermal/config 与给定的明文集合一致；空集合即恢复官方
pub fn sync(plain: &BTreeMap<String, String>, immutable: bool) -> Result<Outcome> {
    let old = installed();
    let mut changed = false;
    for name in old.iter().filter(|n| !plain.contains_key(*n)) {
        remove_one(name);
        changed = true;
    }
    fs::create_dir_all(paths::CFG_DIR)?;
    for (name, text) in plain {
        let bytes = crypto::encrypt(text.as_bytes());
        let dst = format!("{}/{name}", paths::CFG_DIR);
        let ours = old.contains(name);
        if ours && fs::read(&dst).map(|b| b == bytes).unwrap_or(false) {
            chattr(&dst, if immutable { "+i" } else { "-i" });
            continue;
        }
        write_one(name, &bytes, ours, immutable)?;
        changed = true;
    }
    let list: String = plain.keys().map(|k| format!("{k}\n")).collect();
    fs::write(paths::INSTALLED, list)?;
    if changed {
        reload();
        if !plain.is_empty() && !verify() {
            logln!("mi_thermald 无法解析新配置，回滚为官方温控");
            for name in plain.keys() {
                remove_one(name);
            }
            fs::write(paths::INSTALLED, "")?;
            reload();
            bail!("mi_thermald 无法解析生成的配置，已自动恢复官方温控");
        }
    }
    Ok(Outcome { count: plain.len() })
}

/// 检查已安装文件是否被外部修改或删除
pub fn intact(plain: &BTreeMap<String, String>) -> bool {
    plain.iter().all(|(name, text)| fs::read(format!("{}/{name}", paths::CFG_DIR)).map(|b| b == crypto::encrypt(text.as_bytes())).unwrap_or(false))
}
