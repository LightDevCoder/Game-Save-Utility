use crate::config;
use crate::models::{AppError, AppResult};
use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

pub fn log_path() -> AppResult<PathBuf> {
    Ok(config::app_data_dir()?.join("logs").join("app.log"))
}

pub fn init() -> AppResult<()> {
    let path = log_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| AppError::io("日志目录创建失败", parent, err))?;
    }
    info("应用启动");
    Ok(())
}

pub fn info(message: impl AsRef<str>) {
    write_line("INFO", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    write_line("WARN", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write_line("ERROR", message.as_ref());
}

fn write_line(level: &str, message: &str) {
    let Ok(path) = log_path() else {
        return;
    };
    write_line_at(&path, level, message);
}

fn write_line_at(path: &Path, level: &str, message: &str) {
    write_line_at_with_limit(path, level, message, MAX_LOG_BYTES);
}

fn write_line_at_with_limit(path: &Path, level: &str, message: &str, max_bytes: u64) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = rotate_log_if_needed(path, max_bytes);

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let _ = writeln!(file, "[{timestamp}] [{level}] {message}");
}

fn rotate_log_if_needed(path: &Path, max_bytes: u64) -> std::io::Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() <= max_bytes {
        return Ok(());
    }

    let rotated = path.with_extension("log.1");
    if rotated.exists() {
        fs::remove_file(&rotated)?;
    }
    fs::rename(path, rotated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn oversized_log_is_rotated_before_new_line_is_written() {
        let root = tempdir().unwrap();
        let path = root.path().join("app.log");
        fs::write(&path, vec![b'x'; 32]).unwrap();

        write_line_at_with_limit(&path, "INFO", "latest", 16);

        let rotated = path.with_extension("log.1");
        assert!(rotated.exists());
        assert_eq!(fs::metadata(rotated).unwrap().len(), 32);
        assert!(fs::read_to_string(path).unwrap().contains("latest"));
    }
}
