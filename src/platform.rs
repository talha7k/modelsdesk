use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| home().join(".config"))
        .join("modelsdesk")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| home().join(".local").join("share"))
        .join("modelsdesk")
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn hf_hub_cache() -> PathBuf {
    home().join(".cache").join("huggingface").join("hub")
}

pub fn lmstudio_models() -> PathBuf {
    home().join(".lmstudio").join("models")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn state_path() -> PathBuf {
    data_dir().join("state.json")
}

pub fn log_path() -> PathBuf {
    data_dir().join("logs").join("server.log")
}

pub fn os_label() -> &'static str {
    #[cfg(target_os = "macos")]
    return "macos";
    #[cfg(target_os = "linux")]
    return "linux";
    #[cfg(target_os = "windows")]
    return "windows";
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return "unknown";
}

pub fn supported_now() -> bool {
    cfg!(target_os = "macos")
}
