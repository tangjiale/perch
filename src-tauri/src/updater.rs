use serde_json::{json, Value};

#[tauri::command]
pub fn updater_status(app: tauri::AppHandle) -> Value {
    let config = app.config();
    let updater = config.plugins.0.get("updater");
    let endpoint = updater
        .and_then(|v| v.get("endpoints"))
        .and_then(|v| v.as_array())
        .and_then(|v| v.first())
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let key = updater
        .and_then(|v| v.get("pubkey"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let releases_url = endpoint.strip_suffix("/download/latest.json").unwrap_or("");
    json!({
        "configured": !key.is_empty() && endpoint.starts_with("https://"),
        "releasesUrl": releases_url
    })
}

#[tauri::command]
pub fn update_prepare() -> Result<(), String> {
    crate::integrations::ensure_idle()
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_build_has_valid_unconfigured_updater() {
        let app: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let config: tauri_plugin_updater::Config =
            serde_json::from_value(app["plugins"]["updater"].clone()).unwrap();
        assert!(config.pubkey.is_empty());
        assert!(config.endpoints.is_empty());
    }
}
