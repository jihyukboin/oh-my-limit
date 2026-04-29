use std::path::PathBuf;

pub const CONFIG_DIR_NAME: &str = ".oh-my-limit";
pub const CONFIG_FILE_NAME: &str = "config.toml";

pub fn config_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(CONFIG_DIR_NAME)
}

pub fn config_file() -> PathBuf {
    config_dir().join(CONFIG_FILE_NAME)
}
