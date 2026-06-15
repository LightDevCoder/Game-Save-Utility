use crate::config;
use crate::fs_utils::{
    backup_timestamp, copy_dir_recursive, directory_stats, ensure_dir, expand_path,
    is_same_or_child_path, remove_dir_all_if_exists, sanitize_for_path, unique_child_path,
    validate_save_dir,
};
use crate::logger;
use crate::models::{
    AppConfig, AppError, AppResult, BackupEntry, BackupMetadata, GameConfig, TOOL_VERSION,
};
use chrono::{DateTime, Local};
use serde_json::to_string_pretty;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const METADATA_FILE: &str = "metadata.json";
const SAVE_FILES_DIR: &str = "save_files";

pub fn game_backup_dir(config: &AppConfig, game: &GameConfig) -> PathBuf {
    config::expanded_backup_root(config).join(sanitize_for_path(&game.name))
}

pub fn ensure_game_backup_dir(config: &AppConfig, game: &GameConfig) -> AppResult<PathBuf> {
    let dir = game_backup_dir(config, game);
    if dir.exists() && !dir.is_dir() {
        return Err(AppError::message(format!(
            "备份路径不是文件夹: {}",
            dir.display()
        )));
    }
    ensure_dir(&dir, "备份目录创建失败")?;
    Ok(dir)
}

#[cfg(test)]
mod open_dir_tests {
    use super::*;
    use crate::models::AppConfig;
    use tempfile::tempdir;

    #[test]
    fn ensure_game_backup_dir_creates_empty_game_dir() {
        let root = tempdir().unwrap();
        let backup_root = root.path().join("backups");
        let game = GameConfig {
            id: "game-1".to_owned(),
            name: "测试游戏".to_owned(),
            save_path: root.path().join("save"),
            max_backups: None,
            auto_cleanup_enabled: false,
        };
        let config = AppConfig {
            backup_root: backup_root.clone(),
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let dir = ensure_game_backup_dir(&config, &game).unwrap();

        assert_eq!(dir, backup_root.join("测试游戏"));
        assert!(dir.is_dir());
    }
}

pub fn create_backup(
    config: &AppConfig,
    game: &GameConfig,
    label: Option<&str>,
    is_pre_restore_backup: bool,
    allow_empty: bool,
) -> AppResult<BackupEntry> {
    let save_path = expand_path(&game.save_path);
    validate_save_dir(&save_path)?;

    let stats = directory_stats(&save_path)?;
    if stats.file_count == 0 && !allow_empty {
        return Err(AppError::EmptySaveDir { path: save_path });
    }

    let backup_root = ensure_game_backup_dir(config, game)?;
    if is_same_or_child_path(&backup_root, &save_path)? {
        return Err(AppError::message(
            "备份根目录不能放在该游戏的存档目录内部，否则会造成递归备份",
        ));
    }

    let created_at = Local::now();
    let timestamp = backup_timestamp();
    let normalized_label = normalize_label(label, is_pre_restore_backup);
    let dir_name = backup_dir_name(&timestamp, normalized_label.as_deref());
    let final_dir = unique_child_path(&backup_root, &dir_name);
    let temp_dir = backup_root.join(format!(".tmp_{}_{}", dir_name, Uuid::new_v4()));

    logger::info(format!(
        "备份开始: {} -> {}",
        save_path.display(),
        final_dir.display()
    ));

    let result: AppResult<BackupEntry> = (|| {
        let save_files_dir = temp_dir.join(SAVE_FILES_DIR);
        ensure_dir(&save_files_dir, "备份临时目录创建失败")?;
        copy_dir_recursive(&save_path, &save_files_dir)?;

        let metadata = BackupMetadata {
            game_name: game.name.clone(),
            original_save_path: save_path.clone(),
            created_at: created_at.to_rfc3339(),
            label: normalized_label.clone(),
            file_count: stats.file_count,
            total_size: stats.total_size,
            tool_version: TOOL_VERSION.to_owned(),
            is_pre_restore_backup,
        };
        write_metadata(&temp_dir, &metadata)?;

        fs::rename(&temp_dir, &final_dir)
            .map_err(|err| AppError::io("备份目录提交失败", &final_dir, err))?;

        Ok(BackupEntry {
            game_id: game.id.clone(),
            game_name: game.name.clone(),
            path: final_dir,
            created_at,
            label: normalized_label,
            file_count: stats.file_count,
            total_size: stats.total_size,
            is_pre_restore_backup,
        })
    })();

    match result {
        Ok(entry) => {
            logger::info(format!("备份完成: {}", entry.path.display()));
            if game.auto_cleanup_enabled {
                if let Some(max_backups) = game.max_backups {
                    cleanup_old_backups(config, game, max_backups)?;
                }
            }
            Ok(entry)
        }
        Err(err) => {
            let _ = remove_dir_all_if_exists(&temp_dir, "清理失败备份目录失败");
            logger::error(format!("备份失败: {}", err.user_message()));
            Err(err)
        }
    }
}

pub fn scan_backups(config: &AppConfig, game: &GameConfig) -> AppResult<Vec<BackupEntry>> {
    let backup_dir = game_backup_dir(config, game);
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }
    if !backup_dir.is_dir() {
        return Err(AppError::message(format!(
            "备份路径不是文件夹: {}",
            backup_dir.display()
        )));
    }

    let mut entries = Vec::new();
    let dirs = fs::read_dir(&backup_dir)
        .map_err(|err| AppError::io("备份目录读取失败", &backup_dir, err))?;
    for entry in dirs {
        let entry = entry.map_err(|err| AppError::io("备份目录项读取失败", &backup_dir, err))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| AppError::io("备份目录项类型读取失败", &path, err))?;

        if !file_type.is_dir() {
            continue;
        }

        let metadata_path = path.join(METADATA_FILE);
        if !metadata_path.exists() {
            continue;
        }

        match read_backup_entry(game, &path) {
            Ok(backup) => entries.push(backup),
            Err(err) => logger::warn(format!(
                "跳过无效备份节点 {}: {}",
                path.display(),
                err.user_message()
            )),
        }
    }

    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(entries)
}

pub fn delete_backup(entry: &BackupEntry) -> AppResult<()> {
    logger::info(format!("删除备份: {}", entry.path.display()));
    fs::remove_dir_all(&entry.path).map_err(|err| AppError::io("备份删除失败", &entry.path, err))
}

pub fn delete_game_backup_dir(config: &AppConfig, game: &GameConfig) -> AppResult<()> {
    let dir = game_backup_dir(config, game);
    remove_dir_all_if_exists(&dir, "游戏备份目录删除失败")
}

pub fn restore_backup(
    config: &AppConfig,
    game: &GameConfig,
    backup: &BackupEntry,
) -> AppResult<BackupEntry> {
    let backup_save_files = backup.path.join(SAVE_FILES_DIR);
    if !backup.path.exists() {
        return Err(AppError::message(format!(
            "备份节点不存在: {}",
            backup.path.display()
        )));
    }
    if !backup_save_files.exists() || !backup_save_files.is_dir() {
        return Err(AppError::message(format!(
            "备份文件不存在: {}",
            backup_save_files.display()
        )));
    }

    let target = expand_path(&game.save_path);
    if target.exists() && !target.is_dir() {
        return Err(AppError::message(format!(
            "目标存档路径不是文件夹: {}",
            target.display()
        )));
    }
    ensure_dir(&target, "目标存档目录创建失败")?;

    logger::info(format!(
        "恢复开始: {} -> {}",
        backup_save_files.display(),
        target.display()
    ));

    let mut pre_restore_game = game.clone();
    pre_restore_game.auto_cleanup_enabled = false;
    let pre_restore_backup = create_backup(
        config,
        &pre_restore_game,
        Some("恢复前自动备份"),
        true,
        true,
    )?;

    let parent = target
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::message("目标存档路径无父目录，无法安全恢复"))?;
    ensure_dir(&parent, "目标存档父目录创建失败")?;

    let stage_dir = parent.join(format!(
        ".gst_restore_stage_{}",
        Uuid::new_v4().to_string().replace('-', "")
    ));
    let old_dir = parent.join(format!(
        ".gst_restore_old_{}",
        Uuid::new_v4().to_string().replace('-', "")
    ));

    let result: AppResult<()> = (|| {
        copy_dir_recursive(&backup_save_files, &stage_dir)?;

        if target.exists() {
            fs::rename(&target, &old_dir)
                .map_err(|err| AppError::io("当前存档目录移动失败", &target, err))?;
        }

        if let Err(err) = fs::rename(&stage_dir, &target) {
            if !target.exists() && old_dir.exists() {
                let _ = fs::rename(&old_dir, &target);
            }
            return Err(AppError::io(
                format!(
                    "恢复失败，已保留恢复前自动备份 {}",
                    pre_restore_backup.path.display()
                ),
                &target,
                err,
            ));
        }

        let _ = remove_dir_all_if_exists(&old_dir, "清理旧存档临时目录失败");
        Ok(())
    })();

    match result {
        Ok(()) => {
            logger::info(format!("恢复完成: {}", backup.path.display()));
            if game.auto_cleanup_enabled {
                if let Some(max_backups) = game.max_backups {
                    if let Err(err) = cleanup_old_backups(config, game, max_backups) {
                        logger::warn(format!("恢复后自动清理失败: {}", err.user_message()));
                    }
                }
            }
            Ok(pre_restore_backup)
        }
        Err(err) => {
            let _ = remove_dir_all_if_exists(&stage_dir, "清理恢复临时目录失败");
            logger::error(format!(
                "恢复失败: {}; 恢复前自动备份: {}",
                err.user_message(),
                pre_restore_backup.path.display()
            ));
            Err(err)
        }
    }
}

pub fn cleanup_old_backups(
    config: &AppConfig,
    game: &GameConfig,
    max_backups: usize,
) -> AppResult<usize> {
    if max_backups == 0 {
        return Ok(0);
    }

    let entries = scan_backups(config, game)?;
    if entries.len() <= max_backups {
        return Ok(0);
    }

    let mut deleted = 0;
    for entry in entries.into_iter().skip(max_backups) {
        delete_backup(&entry)?;
        deleted += 1;
    }

    Ok(deleted)
}

fn normalize_label(label: Option<&str>, is_pre_restore_backup: bool) -> Option<String> {
    if is_pre_restore_backup {
        return Some("恢复前自动备份".to_owned());
    }

    label
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn backup_dir_name(timestamp: &str, label: Option<&str>) -> String {
    match label {
        Some(label) if !label.trim().is_empty() => {
            format!("{timestamp}_{}", sanitize_for_path(label))
        }
        _ => timestamp.to_owned(),
    }
}

fn write_metadata(dir: &Path, metadata: &BackupMetadata) -> AppResult<()> {
    let path = dir.join(METADATA_FILE);
    let data = to_string_pretty(metadata)?;
    fs::write(&path, data).map_err(|err| AppError::io("备份元数据写入失败", &path, err))
}

fn read_backup_entry(game: &GameConfig, path: &Path) -> AppResult<BackupEntry> {
    let metadata_path = path.join(METADATA_FILE);
    let raw = fs::read_to_string(&metadata_path)
        .map_err(|err| AppError::io("备份元数据读取失败", &metadata_path, err))?;
    let metadata: BackupMetadata = serde_json::from_str(&raw)?;
    let created_at = DateTime::parse_from_rfc3339(&metadata.created_at)
        .map_err(|err| AppError::message(format!("备份时间格式错误: {err}")))?
        .with_timezone(&Local);

    Ok(BackupEntry {
        game_id: game.id.clone(),
        game_name: metadata.game_name,
        path: path.to_path_buf(),
        created_at,
        label: metadata.label,
        file_count: metadata.file_count,
        total_size: metadata.total_size,
        is_pre_restore_backup: metadata.is_pre_restore_backup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AppConfig;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn backup_and_restore_roundtrip() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        let backup_root = root.path().join("backups");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot1.sav"), "before").unwrap();

        let game = GameConfig {
            id: "game-1".to_owned(),
            name: "测试游戏".to_owned(),
            save_path: save_dir.clone(),
            max_backups: None,
            auto_cleanup_enabled: false,
        };
        let config = AppConfig {
            backup_root,
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let first = create_backup(&config, &game, Some("关键选择前"), false, false).unwrap();
        fs::write(save_dir.join("slot1.sav"), "after").unwrap();
        restore_backup(&config, &game, &first).unwrap();

        assert_eq!(
            fs::read_to_string(save_dir.join("slot1.sav")).unwrap(),
            "before"
        );
        assert_eq!(scan_backups(&config, &game).unwrap().len(), 2);
    }

    #[test]
    fn delete_game_backup_dir_removes_sanitized_game_folder() {
        let root = tempdir().unwrap();
        let backup_root = root.path().join("backups");
        let save_dir = root.path().join("save");
        fs::create_dir_all(&save_dir).unwrap();

        let game = GameConfig {
            id: "game-1".to_owned(),
            name: "测试: 游戏".to_owned(),
            save_path: save_dir,
            max_backups: None,
            auto_cleanup_enabled: false,
        };
        let config = AppConfig {
            backup_root,
            games: vec![game.clone()],
            settings: Default::default(),
        };
        let backup_dir = ensure_game_backup_dir(&config, &game).unwrap();
        fs::write(backup_dir.join("marker.txt"), "backup").unwrap();

        delete_game_backup_dir(&config, &game).unwrap();

        assert!(!backup_dir.exists());
    }
}
