use std::path::PathBuf;

const APP_DIR: &str = "github-local-indexer";

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

pub fn database_path() -> PathBuf {
    data_dir().join("index.db")
}
