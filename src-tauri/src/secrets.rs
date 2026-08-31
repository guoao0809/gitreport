//! API key 的安全存储（系统钥匙串）。
//! Windows → 凭据管理器；macOS → 钥匙串；Linux → Secret Service。

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const SERVICE: &str = "com.gitreport.app";
const USERNAME: &str = "ai-api-key";

/// 钥匙串访问超时：Windows 凭据管理器偶发挂起，避免启动被无限阻塞。
const KEYRING_TIMEOUT: Duration = Duration::from_secs(5);

/// 保存 API key 到系统钥匙串
#[tauri::command]
pub fn save_api_key(api_key: String) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, USERNAME)
        .map_err(|e| format!("无法访问系统钥匙串：{e}"))?;
    entry
        .set_password(&api_key)
        .map_err(|e| format!("保存 API key 失败：{e}"))
}

/// 从系统钥匙串读取 API key（未保存时返回 None）。
/// 在子线程执行并限时等待：凭据管理器挂死时超时返回 None，不让启动卡住。
#[tauri::command]
pub fn load_api_key() -> Result<Option<String>, String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // 在子线程做真正的钥匙串访问，主线程只负责限时等待
        let res = (|| -> Result<Option<String>, String> {
            let entry = keyring::Entry::new(SERVICE, USERNAME)
                .map_err(|e| format!("无法访问系统钥匙串：{e}"))?;
            match entry.get_password() {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(format!("读取 API key 失败：{e}")),
            }
        })();
        let _ = tx.send(res);
    });
    // 超时视为未找到 key，让前端正常启动
    match rx.recv_timeout(KEYRING_TIMEOUT) {
        Ok(r) => r,
        Err(_) => Ok(None), // 超时或线程失败 → 当作没有 key
    }
}

/// 从系统钥匙串删除 API key
#[tauri::command]
pub fn delete_api_key() -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, USERNAME)
        .map_err(|e| format!("无法访问系统钥匙串：{e}"))?;
    entry
        .delete_credential()
        .map_err(|e| format!("删除 API key 失败：{e}"))
}
