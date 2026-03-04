use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const SERVICE_NAME: &str = "SingularityMonitorDaemon";
pub const PIPE_NAME: &str = r"\\.\pipe\SingularityMonitor";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub db_path: PathBuf,
    pub log_path: PathBuf,
    pub pipe_name: String,
    pub poll_interval_seconds: u32,
    pub max_delta_bytes: u64,
    pub attribution_mode: String,
    pub trim_working_set: bool,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let data_root = resolve_data_root()?;
        std::fs::create_dir_all(&data_root)
            .with_context(|| format!("failed to create data root at {}", data_root.display()))?;

        let poll_interval_seconds = read_env_u32("SM_POLL_INTERVAL_SECS").unwrap_or(60);
        let poll_interval_seconds = poll_interval_seconds.clamp(15, 300);
        let max_delta_bytes = read_env_u64("SM_MAX_DELTA_BYTES").unwrap_or(10 * 1024 * 1024 * 1024);
        let trim_working_set = std::env::var("SM_TRIM_WORKING_SET")
            .map(|value| value != "0")
            .unwrap_or(true);

        Ok(Self {
            db_path: data_root.join("data.db"),
            log_path: data_root.join("daemon.log.jsonl"),
            pipe_name: PIPE_NAME.to_string(),
            poll_interval_seconds,
            max_delta_bytes,
            attribution_mode: "helper-required".to_string(),
            trim_working_set,
        })
    }
}

fn resolve_data_root() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("SM_DATA_ROOT") {
        return Ok(PathBuf::from(explicit));
    }

    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let preferred = program_data.join("SingularityMonitor");

    if try_create_dir(&preferred).is_ok() {
        return Ok(preferred);
    }

    let fallback = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SingularityMonitorDev");
    try_create_dir(&fallback).with_context(|| {
        format!(
            "failed to create fallback data root at {}",
            fallback.display()
        )
    })?;
    Ok(fallback)
}

fn try_create_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("unable to create directory {}", path.display()))
}

fn read_env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok()?.parse::<u32>().ok()
}

fn read_env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.parse::<u64>().ok()
}
