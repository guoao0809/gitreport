//! API key 的安全存储（系统钥匙串）。
//! Windows → 凭据管理器；macOS → 钥匙串；Linux → Secret Service。

const SERVICE: &str = "com.gitreport.app";
const USERNAME: &str = "ai-api-key";

/// 保存 API key 到系统钥匙串
#[tauri::command]
pub fn save_api_key(api_key: String) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, USERNAME)
        .map_err(|e| format!("无法访问系统钥匙串：{e}"))?;
    entry
        .set_password(&api_key)
        .map_err(|e| format!("保存 API key 失败：{e}"))
}

/// 从系统钥匙串读取 API key（未保存时返回 None）
#[tauri::command]
pub fn load_api_key() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, USERNAME)
        .map_err(|e| format!("无法访问系统钥匙串：{e}"))?;
    match entry.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取 API key 失败：{e}")),
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
