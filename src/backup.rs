use crate::fs_utils::{
    backup_timestamp, copy_dir_recursive, directory_stats, ensure_dir, expand_path,
    is_same_or_child_path, remove_dir_all_if_exists, sanitize_for_path, unique_child_path,
    validate_save_dir,
};
use crate::logger;
use crate::models::{
    AppConfig, AppError, AppResult, BackupEntry, BackupMetadata, BackupStorageKind,
    BackupStorageMode, GameConfig, IncrementalBackupKind, TOOL_VERSION,
};
use crate::{archive, config, snapshot};
use chrono::{DateTime, Local};
use serde_json::to_string_pretty;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const METADATA_FILE: &str = "metadata.json";
const SAVE_FILES_DIR: &str = "save_files";

#[derive(Clone, Debug)]
struct BackupPayloadSummary {
    file_count: u64,
    total_size: u64,
    stored_size: Option<u64>,
    storage_kind: BackupStorageKind,
    manifest_path: Option<PathBuf>,
    archive_path: Option<PathBuf>,
    snapshot_hash: Option<String>,
    incremental_kind: Option<IncrementalBackupKind>,
}

pub fn game_backup_dir(config: &AppConfig, game: &GameConfig) -> PathBuf {
    config::expanded_backup_root(config).join(sanitize_for_path(&game.name))
}

pub fn ensure_game_backup_dir(config: &AppConfig, game: &GameConfig) -> AppResult<PathBuf> {
    let dir = game_backup_dir(config, game);
    if dir.exists() && !dir.is_dir() {
        return Err(AppError::message(format!(
            "Backup path is not a folder: {}",
            dir.display()
        )));
    }
    ensure_dir(&dir, "Backup folder create failed")?;
    Ok(dir)
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
            "Backup root cannot be inside this game's save folder; that would recurse forever",
        ));
    }

    let created_at = Local::now();
    let timestamp = backup_timestamp();
    let normalized_label = normalize_label(label, is_pre_restore_backup);
    let dir_name = backup_dir_name(&timestamp, normalized_label.as_deref());
    let final_dir = unique_child_path(&backup_root, &dir_name);
    let temp_dir = backup_root.join(format!(".tmp_{}_{}", dir_name, Uuid::new_v4()));

    logger::info(format!(
        "Backup started: {} -> {}",
        save_path.display(),
        final_dir.display()
    ));

    let result: AppResult<BackupEntry> = (|| {
        ensure_dir(&temp_dir, "Backup temp folder create failed")?;
        let storage_summary = create_backup_payload(&save_path, &backup_root, &temp_dir, game)?;

        let metadata = BackupMetadata {
            game_name: game.name.clone(),
            original_save_path: save_path.clone(),
            created_at: created_at.to_rfc3339(),
            label: normalized_label.clone(),
            file_count: storage_summary.file_count,
            total_size: storage_summary.total_size,
            tool_version: TOOL_VERSION.to_owned(),
            is_pre_restore_backup,
            storage_kind: storage_summary.storage_kind,
            manifest_path: storage_summary.manifest_path,
            archive_path: storage_summary.archive_path,
            snapshot_hash: storage_summary.snapshot_hash,
            stored_size: storage_summary.stored_size,
            incremental_kind: storage_summary.incremental_kind,
        };
        write_metadata(&temp_dir, &metadata)?;

        fs::rename(&temp_dir, &final_dir)
            .map_err(|err| AppError::io("Backup folder commit failed", &final_dir, err))?;

        Ok(BackupEntry {
            game_id: game.id.clone(),
            game_name: game.name.clone(),
            path: final_dir,
            created_at,
            label: normalized_label,
            file_count: storage_summary.file_count,
            total_size: storage_summary.total_size,
            stored_size: storage_summary.stored_size,
            is_pre_restore_backup,
            storage_kind: storage_summary.storage_kind,
            incremental_kind: storage_summary.incremental_kind,
        })
    })();

    match result {
        Ok(entry) => {
            logger::info(format!("Backup complete: {}", entry.path.display()));
            if game.auto_cleanup_enabled {
                if let Some(max_backups) = game.max_backups {
                    cleanup_old_backups(config, game, max_backups)?;
                }
            }
            Ok(entry)
        }
        Err(err) => {
            let _ = remove_dir_all_if_exists(&temp_dir, "Failed backup temp cleanup failed");
            logger::error(format!("Backup failed: {}", err.user_message()));
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
            "Backup path is not a folder: {}",
            backup_dir.display()
        )));
    }

    let mut entries = Vec::new();
    let dirs = fs::read_dir(&backup_dir)
        .map_err(|err| AppError::io("Backup folder read failed", &backup_dir, err))?;
    for entry in dirs {
        let entry = entry
            .map_err(|err| AppError::io("Backup folder entry read failed", &backup_dir, err))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| AppError::io("Backup folder entry type read failed", &path, err))?;

        if !file_type.is_dir() {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some(snapshot::OBJECT_STORE_DIR) {
            continue;
        }

        let metadata_path = path.join(METADATA_FILE);
        if !metadata_path.exists() {
            continue;
        }

        match read_backup_entry(game, &path) {
            Ok(backup) => entries.push(backup),
            Err(err) => logger::warn(format!(
                "Skipping invalid backup node {}: {}",
                path.display(),
                err.user_message()
            )),
        }
    }

    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    infer_missing_incremental_kinds(&mut entries);
    Ok(entries)
}

pub fn delete_backup(entry: &BackupEntry) -> AppResult<()> {
    logger::info(format!("Delete backup: {}", entry.path.display()));
    fs::remove_dir_all(&entry.path)
        .map_err(|err| AppError::io("Backup delete failed", &entry.path, err))?;
    if let Some(game_backup_dir) = entry.path.parent() {
        let _ = snapshot::garbage_collect_objects(game_backup_dir);
    }
    Ok(())
}

pub fn delete_game_backup_dir(config: &AppConfig, game: &GameConfig) -> AppResult<()> {
    let dir = game_backup_dir(config, game);
    remove_dir_all_if_exists(&dir, "Game backup folder delete failed")
}

pub fn restore_backup(
    config: &AppConfig,
    game: &GameConfig,
    backup: &BackupEntry,
) -> AppResult<BackupEntry> {
    if !backup.path.exists() {
        return Err(AppError::message(format!(
            "Backup node does not exist: {}",
            backup.path.display()
        )));
    }
    validate_backup_payload(backup)?;

    let target = expand_path(&game.save_path);
    if target.exists() && !target.is_dir() {
        return Err(AppError::message(format!(
            "Target save path is not a folder: {}",
            target.display()
        )));
    }
    ensure_dir(&target, "Target save folder create failed")?;

    logger::info(format!(
        "Restore started: {} -> {}",
        backup.path.display(),
        target.display()
    ));

    let mut pre_restore_game = game.clone();
    pre_restore_game.auto_cleanup_enabled = false;
    let pre_restore_backup = create_backup(
        config,
        &pre_restore_game,
        Some("Pre-restore automatic backup"),
        true,
        true,
    )?;

    let parent = target
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::message("Target save path has no parent folder"))?;
    ensure_dir(&parent, "Target save parent folder create failed")?;

    let stage_dir = parent.join(format!(
        ".gst_restore_stage_{}",
        Uuid::new_v4().to_string().replace('-', "")
    ));
    let old_dir = parent.join(format!(
        ".gst_restore_old_{}",
        Uuid::new_v4().to_string().replace('-', "")
    ));

    let result: AppResult<()> = (|| {
        materialize_backup_payload(backup, &stage_dir)?;

        if target.exists() {
            fs::rename(&target, &old_dir)
                .map_err(|err| AppError::io("Current save folder move failed", &target, err))?;
        }

        if let Err(err) = fs::rename(&stage_dir, &target) {
            if !target.exists() && old_dir.exists() {
                let _ = fs::rename(&old_dir, &target);
            }
            return Err(AppError::io(
                format!(
                    "Restore failed; pre-restore safety backup kept at {}",
                    pre_restore_backup.path.display()
                ),
                &target,
                err,
            ));
        }

        let _ = remove_dir_all_if_exists(&old_dir, "Old save temp cleanup failed");
        Ok(())
    })();

    match result {
        Ok(()) => {
            logger::info(format!("Restore complete: {}", backup.path.display()));
            if game.auto_cleanup_enabled {
                if let Some(max_backups) = game.max_backups {
                    if let Err(err) = cleanup_old_backups(config, game, max_backups) {
                        logger::warn(format!(
                            "Post-restore cleanup failed: {}",
                            err.user_message()
                        ));
                    }
                }
            }
            Ok(pre_restore_backup)
        }
        Err(err) => {
            let _ = remove_dir_all_if_exists(&stage_dir, "Restore temp cleanup failed");
            logger::error(format!(
                "Restore failed: {}; pre-restore safety backup: {}",
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

pub fn latest_snapshot_hash(config: &AppConfig, game: &GameConfig) -> AppResult<Option<String>> {
    Ok(scan_backups(config, game)?
        .into_iter()
        .find_map(|entry| read_metadata(&entry.path).ok()?.snapshot_hash))
}

fn create_backup_payload(
    save_path: &Path,
    game_backup_dir: &Path,
    temp_dir: &Path,
    game: &GameConfig,
) -> AppResult<BackupPayloadSummary> {
    match game.backup_storage_mode {
        BackupStorageMode::Incremental => {
            let incremental_kind = if has_existing_incremental_backups(game_backup_dir)? {
                IncrementalBackupKind::Incremental
            } else {
                IncrementalBackupKind::Full
            };
            let summary =
                snapshot::create_incremental_snapshot(save_path, game_backup_dir, temp_dir)?;
            Ok(BackupPayloadSummary {
                file_count: summary.file_count,
                total_size: summary.total_size,
                stored_size: Some(summary.stored_size),
                storage_kind: BackupStorageKind::Incremental,
                manifest_path: Some(PathBuf::from(snapshot::MANIFEST_FILE)),
                archive_path: None,
                snapshot_hash: Some(summary.snapshot_hash),
                incremental_kind: Some(incremental_kind),
            })
        }
        BackupStorageMode::Zip => {
            let archive_path = temp_dir.join(archive::ARCHIVE_FILE);
            let summary = archive::create_zip_backup(save_path, &archive_path)?;
            let snapshot_hash = snapshot::calculate_directory_snapshot_hash(save_path)?;
            Ok(BackupPayloadSummary {
                file_count: summary.file_count,
                total_size: summary.total_size,
                stored_size: Some(summary.stored_size),
                storage_kind: BackupStorageKind::Zip,
                manifest_path: None,
                archive_path: Some(PathBuf::from(archive::ARCHIVE_FILE)),
                snapshot_hash,
                incremental_kind: None,
            })
        }
    }
}

fn has_existing_incremental_backups(game_backup_dir: &Path) -> AppResult<bool> {
    if !game_backup_dir.exists() {
        return Ok(false);
    }
    for entry in fs::read_dir(game_backup_dir)
        .map_err(|err| AppError::io("Game backup folder read failed", game_backup_dir, err))?
    {
        let entry = entry
            .map_err(|err| AppError::io("Game backup entry read failed", game_backup_dir, err))?;
        let path = entry.path();
        if !path.is_dir()
            || path.file_name().and_then(|name| name.to_str()) == Some(snapshot::OBJECT_STORE_DIR)
        {
            continue;
        }
        let metadata_path = path.join(METADATA_FILE);
        if !metadata_path.exists() {
            continue;
        }
        if read_metadata(&path)
            .map(|metadata| metadata.storage_kind == BackupStorageKind::Incremental)
            .unwrap_or(false)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_backup_payload(backup: &BackupEntry) -> AppResult<()> {
    match backup.storage_kind {
        BackupStorageKind::LegacyDirectory => {
            let save_files = backup.path.join(SAVE_FILES_DIR);
            if !save_files.is_dir() {
                return Err(AppError::message(format!(
                    "Backup files folder does not exist: {}",
                    save_files.display()
                )));
            }
        }
        BackupStorageKind::Incremental => {
            let manifest = backup.path.join(snapshot::MANIFEST_FILE);
            if !manifest.is_file() {
                return Err(AppError::message(format!(
                    "Incremental backup manifest does not exist: {}",
                    manifest.display()
                )));
            }
        }
        BackupStorageKind::Zip => {
            let archive = backup.path.join(archive::ARCHIVE_FILE);
            if !archive.is_file() {
                return Err(AppError::message(format!(
                    "ZIP backup file does not exist: {}",
                    archive.display()
                )));
            }
        }
    }
    Ok(())
}

fn materialize_backup_payload(backup: &BackupEntry, stage_dir: &Path) -> AppResult<()> {
    match backup.storage_kind {
        BackupStorageKind::LegacyDirectory => {
            copy_dir_recursive(&backup.path.join(SAVE_FILES_DIR), stage_dir)
        }
        BackupStorageKind::Incremental => {
            snapshot::materialize_incremental_backup(&backup.path, stage_dir)
        }
        BackupStorageKind::Zip => {
            archive::extract_zip_backup(&backup.path.join(archive::ARCHIVE_FILE), stage_dir)
        }
    }
}

fn normalize_label(label: Option<&str>, is_pre_restore_backup: bool) -> Option<String> {
    if is_pre_restore_backup {
        return Some("Pre-restore automatic backup".to_owned());
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
    fs::write(&path, data).map_err(|err| AppError::io("Backup metadata write failed", &path, err))
}

fn read_metadata(path: &Path) -> AppResult<BackupMetadata> {
    let metadata_path = path.join(METADATA_FILE);
    let raw = fs::read_to_string(&metadata_path)
        .map_err(|err| AppError::io("Backup metadata read failed", &metadata_path, err))?;
    Ok(serde_json::from_str(&raw)?)
}

fn read_backup_entry(game: &GameConfig, path: &Path) -> AppResult<BackupEntry> {
    let metadata = read_metadata(path)?;
    let created_at = DateTime::parse_from_rfc3339(&metadata.created_at)
        .map_err(|err| AppError::message(format!("Backup timestamp format is invalid: {err}")))?
        .with_timezone(&Local);

    Ok(BackupEntry {
        game_id: game.id.clone(),
        game_name: metadata.game_name,
        path: path.to_path_buf(),
        created_at,
        label: metadata.label,
        file_count: metadata.file_count,
        total_size: metadata.total_size,
        stored_size: metadata.stored_size,
        is_pre_restore_backup: metadata.is_pre_restore_backup,
        storage_kind: metadata.storage_kind,
        incremental_kind: metadata.incremental_kind,
    })
}

fn infer_missing_incremental_kinds(entries: &mut [BackupEntry]) {
    let mut incremental_indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (entry.storage_kind == BackupStorageKind::Incremental
                && entry.incremental_kind.is_none())
            .then_some(index)
        })
        .collect();

    incremental_indices.sort_by_key(|index| entries[*index].created_at);
    for (position, index) in incremental_indices.into_iter().enumerate() {
        entries[index].incremental_kind = Some(if position == 0 {
            IncrementalBackupKind::Full
        } else {
            IncrementalBackupKind::Incremental
        });
    }
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
            name: "Test Game".to_owned(),
            save_path: root.path().join("save"),
            max_backups: None,
            auto_cleanup_enabled: false,
            backup_storage_mode: BackupStorageMode::Incremental,
            steam_link: None,
            auto_backup: Default::default(),
        };
        let config = AppConfig {
            backup_root: backup_root.clone(),
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let dir = ensure_game_backup_dir(&config, &game).unwrap();

        assert_eq!(dir, backup_root.join("Test Game"));
        assert!(dir.is_dir());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AppConfig;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn backup_and_restore_roundtrip_with_incremental_storage() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        let backup_root = root.path().join("backups");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot1.sav"), "before").unwrap();

        let game = test_game(
            "game-1",
            "Test Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let config = AppConfig {
            backup_root,
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let first = create_backup(&config, &game, Some("before choice"), false, false).unwrap();
        assert_eq!(first.storage_kind, BackupStorageKind::Incremental);

        fs::write(save_dir.join("slot1.sav"), "after").unwrap();
        restore_backup(&config, &game, &first).unwrap();

        assert_eq!(
            fs::read_to_string(save_dir.join("slot1.sav")).unwrap(),
            "before"
        );
        assert_eq!(scan_backups(&config, &game).unwrap().len(), 2);
    }

    #[test]
    fn legacy_directory_backup_still_restores() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        let backup_root = root.path().join("backups");
        let game_dir = backup_root.join("Legacy Game");
        let backup_dir = game_dir.join("2026-01-01_00-00-00");
        fs::create_dir_all(backup_dir.join(SAVE_FILES_DIR)).unwrap();
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot.sav"), "current").unwrap();
        fs::write(backup_dir.join(SAVE_FILES_DIR).join("slot.sav"), "legacy").unwrap();

        let game = test_game(
            "game-1",
            "Legacy Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let metadata = BackupMetadata {
            game_name: game.name.clone(),
            original_save_path: save_dir.clone(),
            created_at: Local::now().to_rfc3339(),
            label: None,
            file_count: 1,
            total_size: 6,
            tool_version: "0.1.0".to_owned(),
            is_pre_restore_backup: false,
            storage_kind: BackupStorageKind::LegacyDirectory,
            manifest_path: None,
            archive_path: None,
            snapshot_hash: None,
            stored_size: None,
            incremental_kind: None,
        };
        write_metadata(&backup_dir, &metadata).unwrap();
        let config = AppConfig {
            backup_root,
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let entry = scan_backups(&config, &game).unwrap().remove(0);
        restore_backup(&config, &game, &entry).unwrap();

        assert_eq!(
            fs::read_to_string(save_dir.join("slot.sav")).unwrap(),
            "legacy"
        );
    }

    #[test]
    fn zip_backup_roundtrips_through_restore() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        let backup_root = root.path().join("backups");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot1.sav"), "before").unwrap();

        let game = test_game("game-1", "Zip Game", &save_dir, BackupStorageMode::Zip);
        let config = AppConfig {
            backup_root,
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let first = create_backup(&config, &game, None, false, false).unwrap();
        assert_eq!(first.storage_kind, BackupStorageKind::Zip);
        assert!(first.stored_size.is_some());

        fs::write(save_dir.join("slot1.sav"), "after").unwrap();
        restore_backup(&config, &game, &first).unwrap();

        assert_eq!(
            fs::read_to_string(save_dir.join("slot1.sav")).unwrap(),
            "before"
        );
    }

    #[test]
    fn deleting_incremental_backup_collects_unreferenced_objects() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        let backup_root = root.path().join("backups");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot1.sav"), "one").unwrap();

        let game = test_game(
            "game-1",
            "GC Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let config = AppConfig {
            backup_root: backup_root.clone(),
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let backup = create_backup(&config, &game, None, false, false).unwrap();
        let game_dir = game_backup_dir(&config, &game);
        assert!(game_dir.join(snapshot::OBJECT_STORE_DIR).exists());

        delete_backup(&backup).unwrap();
        let object_files = walkdir::WalkDir::new(game_dir.join(snapshot::OBJECT_STORE_DIR))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .count();
        assert_eq!(object_files, 0);
    }

    #[test]
    fn first_incremental_backup_is_marked_full_then_later_incremental() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        let backup_root = root.path().join("backups");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot1.sav"), "one").unwrap();

        let game = test_game(
            "game-1",
            "Kind Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let config = AppConfig {
            backup_root,
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let first = create_backup(&config, &game, Some("first"), false, false).unwrap();
        assert_eq!(first.incremental_kind, Some(IncrementalBackupKind::Full));

        fs::write(save_dir.join("slot1.sav"), "two").unwrap();
        let second = create_backup(&config, &game, Some("second"), false, false).unwrap();
        assert_eq!(
            second.incremental_kind,
            Some(IncrementalBackupKind::Incremental)
        );

        let backups = scan_backups(&config, &game).unwrap();
        let scanned_first = backups
            .iter()
            .find(|backup| backup.path == first.path)
            .unwrap();
        let scanned_second = backups
            .iter()
            .find(|backup| backup.path == second.path)
            .unwrap();
        assert_eq!(
            scanned_first.incremental_kind,
            Some(IncrementalBackupKind::Full)
        );
        assert_eq!(
            scanned_second.incremental_kind,
            Some(IncrementalBackupKind::Incremental)
        );
    }

    #[test]
    fn delete_game_backup_dir_removes_sanitized_game_folder() {
        let root = tempdir().unwrap();
        let backup_root = root.path().join("backups");
        let save_dir = root.path().join("save");
        fs::create_dir_all(&save_dir).unwrap();

        let game = test_game(
            "game-1",
            "Test: Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
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

    fn test_game(id: &str, name: &str, save_dir: &Path, mode: BackupStorageMode) -> GameConfig {
        GameConfig {
            id: id.to_owned(),
            name: name.to_owned(),
            save_path: save_dir.to_path_buf(),
            max_backups: None,
            auto_cleanup_enabled: false,
            backup_storage_mode: mode,
            steam_link: None,
            auto_backup: Default::default(),
        }
    }
}
