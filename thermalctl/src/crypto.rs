//! mi_thermald 配置加解密：AES-128-CBC，key = iv，PKCS#7 填充
//!
//! 不同机型使用不同的密钥：旧机型为 "thermalopenssl.h"，部分新机型改用另一把二进制密钥。
//! 解密时逐个尝试并校验明文；加密时使用本机官方配置所用的那把。

use aes::Aes128;
use anyhow::{Result, anyhow};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use std::fs;
use std::sync::OnceLock;

pub const KEYS: [&[u8; 16]; 2] = [
    b"thermalopenssl.h",
    &[0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17, 0x2a],
];

const PROBE: &[&str] = &["/odm/etc/thermal-normal.conf", "/vendor/etc/thermal-normal.conf"];

static ACTIVE: OnceLock<usize> = OnceLock::new();

fn decrypt_with(key: &[u8; 16], cipher: &[u8]) -> Option<Vec<u8>> {
    cbc::Decryptor::<Aes128>::new(key.into(), key.into()).decrypt_padded_vec_mut::<Pkcs7>(cipher).ok()
}

/// 明文是否像温控配置：以段名开头且几乎全是可打印字符
fn plausible(plain: &[u8]) -> bool {
    let text = String::from_utf8_lossy(plain);
    let printable = text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t')).count();
    text.trim_start().starts_with('[') && printable * 100 >= text.chars().count() * 98
}

/// 用所有已知密钥尝试解密，返回 (密钥序号, 明文)
pub fn decrypt_any(cipher: &[u8]) -> Option<(usize, Vec<u8>)> {
    let first = ACTIVE.get().copied().unwrap_or(0);
    std::iter::once(first)
        .chain((0..KEYS.len()).filter(|i| *i != first))
        .find_map(|i| decrypt_with(KEYS[i], cipher).filter(|p| plausible(p)).map(|p| (i, p)))
}

/// 本机官方配置使用的密钥序号（首次调用时探测并缓存）
pub fn active_key() -> usize {
    *ACTIVE.get_or_init(|| {
        PROBE.iter().filter_map(|p| fs::read(p).ok()).find_map(|c| decrypt_any(&c).map(|(i, _)| i)).unwrap_or(0)
    })
}

pub fn encrypt(plain: &[u8]) -> Vec<u8> {
    let key = KEYS[active_key()];
    cbc::Encryptor::<Aes128>::new(key.into(), key.into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
}

pub fn decrypt(cipher: &[u8]) -> Result<Vec<u8>> {
    decrypt_any(cipher).map(|(_, p)| p).ok_or_else(|| anyhow!("解密失败：数据不是有效的温控配置"))
}

pub fn decrypt_text(cipher: &[u8]) -> Result<String> {
    Ok(String::from_utf8_lossy(&decrypt(cipher)?).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "[SS-CPU7]\nalgo_type\tss\nsensor\tVIRTUAL-SENSOR0\ndevice\tcpu7\ntrig\t45000\nclr\t43000\ntarget\t2745600\n";

    fn enc_with(key: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        cbc::Encryptor::<Aes128>::new(key.into(), key.into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
    }

    #[test]
    fn detects_each_key() {
        for (i, key) in KEYS.iter().enumerate() {
            let (found, plain) = decrypt_any(&enc_with(key, SAMPLE.as_bytes())).expect("decrypts");
            assert_eq!(found, i);
            assert_eq!(plain, SAMPLE.as_bytes());
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(decrypt_any(&[0u8; 64]).is_none());
        assert!(decrypt_any(SAMPLE.as_bytes()).is_none());
    }
}
