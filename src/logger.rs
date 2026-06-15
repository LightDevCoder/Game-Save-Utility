use crate::config;
use crate::models::{AppError, AppResult};
use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

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

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let _ = writeln!(file, "[{timestamp}] [{level}] {message}");
}
