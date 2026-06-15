use crate::models::{AppError, AppResult};
use chrono::Local;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default)]
pub struct FileStats {
    pub file_count: u64,
    pub total_size: u64,
}

pub fn expand_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    PathBuf::from(expand_env_vars(raw.as_ref()))
}

pub fn expand_env_vars(input: &str) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            output.push(ch);
            continue;
        }

        let mut name = String::new();
        while let Some(&next) = chars.peek() {
            chars.next();
            if next == '%' {
                break;
            }
            name.push(next);
        }

        if name.is_empty() {
            output.push('%');
            continue;
        }

        match env::var(&name) {
            Ok(value) => output.push_str(&value),
            Err(_) => {
                output.push('%');
                output.push_str(&name);
                output.push('%');
            }
        }
    }

    output
}

pub fn sanitize_for_path(name: &str) -> String {
    let mut clean = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            clean.push('_');
        } else {
            clean.push(ch);
        }
    }

    let clean = clean.trim_matches(|ch| ch == ' ' || ch == '.').trim();
    if clean.is_empty() {
        "未命名游戏".to_owned()
    } else {
        clean.to_owned()
    }
}

pub fn backup_timestamp() -> String {
    Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{size:.2} {}", UNITS[unit])
    }
}

pub fn directory_stats(path: &Path) -> AppResult<FileStats> {
    let mut stats = FileStats::default();
    collect_stats(path, &mut stats)?;
    Ok(stats)
}

fn collect_stats(path: &Path, stats: &mut FileStats) -> AppResult<()> {
    let entries = fs::read_dir(path).map_err(|err| AppError::io("目录读取失败", path, err))?;

    for entry in entries {
        let entry = entry.map_err(|err| AppError::io("目录项读取失败", path, err))?;
        let entry_path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| AppError::io("文件类型读取失败", &entry_path, err))?;

        if file_type.is_symlink() {
            return Err(AppError::message(format!(
                "暂不支持包含符号链接的存档目录: {}",
                entry_path.display()
            )));
        }

        if file_type.is_dir() {
            collect_stats(&entry_path, stats)?;
        } else if file_type.is_file() {
            let metadata = entry
                .metadata()
                .map_err(|err| AppError::io("文件信息读取失败", &entry_path, err))?;
            stats.file_count += 1;
            stats.total_size += metadata.len();
        }
    }

    Ok(())
}

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> AppResult<()> {
    fs::create_dir_all(dst).map_err(|err| AppError::io("目录创建失败", dst, err))?;
    copy_dir_contents(src, dst)
}

fn copy_dir_contents(src: &Path, dst: &Path) -> AppResult<()> {
    let entries = fs::read_dir(src).map_err(|err| AppError::io("源目录读取失败", src, err))?;

    for entry in entries {
        let entry = entry.map_err(|err| AppError::io("源目录项读取失败", src, err))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|err| AppError::io("文件类型读取失败", &src_path, err))?;

        if file_type.is_symlink() {
            return Err(AppError::message(format!(
                "暂不支持复制符号链接: {}",
                src_path.display()
            )));
        }

        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)
                .map_err(|err| AppError::io("目录创建失败", &dst_path, err))?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| AppError::io("目录创建失败", parent, err))?;
            }
            fs::copy(&src_path, &dst_path).map_err(|err| {
                AppError::io(
                    format!(
                        "文件复制失败，请确认游戏已关闭且磁盘空间足够，源文件 {}",
                        src_path.display()
                    ),
                    &dst_path,
                    err,
                )
            })?;
        }
    }

    Ok(())
}

pub fn ensure_dir(path: &Path, context: &str) -> AppResult<()> {
    fs::create_dir_all(path).map_err(|err| AppError::io(context, path, err))
}

pub fn remove_dir_all_if_exists(path: &Path, context: &str) -> AppResult<()> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|err| AppError::io(context, path, err))?;
    }
    Ok(())
}

pub fn is_same_or_child_path(path: &Path, parent: &Path) -> AppResult<bool> {
    let path = path
        .canonicalize()
        .map_err(|err| AppError::io("路径规范化失败", path, err))?;
    let parent = parent
        .canonicalize()
        .map_err(|err| AppError::io("路径规范化失败", parent, err))?;

    Ok(path == parent || path.starts_with(parent))
}

pub fn unique_child_path(parent: &Path, base_name: &str) -> PathBuf {
    let candidate = parent.join(base_name);
    if !candidate.exists() {
        return candidate;
    }

    for index in 1..1000 {
        let candidate = parent.join(format!("{base_name}_{index}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!("{base_name}_{}", uuid::Uuid::new_v4()))
}

pub fn validate_save_dir(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Err(AppError::message(format!(
            "存档路径不存在: {}",
            path.display()
        )));
    }
    if !path.is_dir() {
        return Err(AppError::message(format!(
            "存档路径不是文件夹: {}",
            path.display()
        )));
    }

    fs::read_dir(path).map_err(|err| AppError::io("没有读取权限或目录不可访问", path, err))?;
    Ok(())
}
