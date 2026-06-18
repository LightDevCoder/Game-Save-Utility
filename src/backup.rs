use crate::fs_utils::{
    backup_timestamp, copy_dir_recursive, directory_stats, ensure_dir, expand_path,
    is_same_or_child_path, remove_dir_all_if_exists, sanitize_for_path, unique_child_path,
    validate_save_dir,
};
use crate::logger;
use crate::models::{
    system_backup_label, AppConfig, AppError, AppResult, BackupEntry, BackupLabelKind,
    BackupMetadata, BackupStorageKind, BackupStorageMode, GameConfig, IncrementalBackupKind,
    Language, TOOL_VERSION,
};
use crate::{archive, config, snapshot};
use chrono::{DateTime, Local};
use serde_json::{to_string_pretty, Value};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const METADATA_FILE: &str = "metadata.json";
const SAVE_FILES_DIR: &str = "save_files";

#[derive(Clone, Copy, Debug)]
pub struct BackupCreateOptions<'a> {
    pub label: Option<&'a str>,
    pub label_kind: BackupLabelKind,
    pub language: Language,
    pub allow_empty: bool,
}

impl<'a> BackupCreateOptions<'a> {
    pub fn manual(label: Option<&'a str>, language: Language, allow_empty: bool) -> Self {
        Self {
            label,
            label_kind: BackupLabelKind::Manual,
            language,
            allow_empty,
        }
    }

    pub fn automatic(language: Language) -> Self {
        Self {
            label: None,
            label_kind: BackupLabelKind::Automatic,
            language,
            allow_empty: false,
        }
    }

    pub fn pre_restore(language: Language) -> Self {
        Self {
            label: None,
            label_kind: BackupLabelKind::PreRestore,
            language,
            allow_empty: true,
        }
    }
}

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
    options: BackupCreateOptions<'_>,
) -> AppResult<BackupEntry> {
    let save_path = expand_path(&game.save_path);
    validate_save_dir(&save_path)?;

    let stats = directory_stats(&save_path)?;
    if stats.file_count == 0 && !options.allow_empty {
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
    let normalized_label = normalize_label(options);
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
            is_pre_restore_backup: options.label_kind == BackupLabelKind::PreRestore,
            label_kind: options.label_kind,
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
            is_pre_restore_backup: options.label_kind == BackupLabelKind::PreRestore,
            label_kind: options.label_kind,
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
            logger::error(format!("Backup failed: {err}"));
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
                err
            )),
        }
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
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
        BackupCreateOptions::pre_restore(config.settings.language),
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
                    if let Err(err) =
                        cleanup_old_backups_inner(config, game, max_backups, Some(&backup.path))
                    {
                        logger::warn(format!("Post-restore cleanup failed: {err}"));
                    }
                }
            }
            Ok(pre_restore_backup)
        }
        Err(err) => {
            let _ = remove_dir_all_if_exists(&stage_dir, "Restore temp cleanup failed");
            logger::error(format!(
                "Restore failed: {}; pre-restore safety backup: {}",
                err,
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
    cleanup_old_backups_inner(config, game, max_backups, None)
}

fn cleanup_old_backups_inner(
    config: &AppConfig,
    game: &GameConfig,
    max_backups: usize,
    protected_path: Option<&Path>,
) -> AppResult<usize> {
    if max_backups == 0 {
        return Ok(0);
    }

    let automatic_backups: Vec<BackupEntry> = scan_backups(config, game)?
        .into_iter()
        .filter(|entry| entry.label_kind == BackupLabelKind::Automatic)
        .collect();
    if automatic_backups.len() <= max_backups {
        return Ok(0);
    }

    let mut deleted = 0;
    for entry in automatic_backups.into_iter().skip(max_backups) {
        if protected_path.is_some_and(|path| path == entry.path) {
            continue;
        }
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

fn normalize_label(options: BackupCreateOptions<'_>) -> Option<String> {
    if let Some(label) = system_backup_label(options.label_kind, options.language) {
        return Some(label.to_owned());
    }

    options
        .label
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
    let value: Value = serde_json::from_str(&raw)?;
    let has_label_kind = value.get("label_kind").is_some();
    let mut metadata: BackupMetadata = serde_json::from_value(value)?;
    if !has_label_kind {
        metadata.label_kind = infer_legacy_label_kind(&metadata);
    }
    Ok(metadata)
}

fn infer_legacy_label_kind(metadata: &BackupMetadata) -> BackupLabelKind {
    if metadata.is_pre_restore_backup
        || matches!(
            metadata.label.as_deref(),
            Some("Pre-restore automatic backup" | "恢复前自动备份")
        )
    {
        BackupLabelKind::PreRestore
    } else if matches!(
        metadata.label.as_deref(),
        Some("Automatic backup" | "自动备份")
    ) {
        BackupLabelKind::Automatic
    } else {
        BackupLabelKind::Manual
    }
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
        is_pre_restore_backup: metadata.label_kind == BackupLabelKind::PreRestore,
        label_kind: metadata.label_kind,
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

        let first = create_backup(
            &config,
            &game,
            BackupCreateOptions::manual(Some("before choice"), Language::EnUs, false),
        )
        .unwrap();
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
            label_kind: BackupLabelKind::Manual,
            storage_kind: BackupStorageKind::LegacyDirectory,
            manifest_path: None,
            archive_path: None,
            snapshot_hash: None,
            stored_size: None,
            incremental_kind: None,
        };
        let mut legacy_value = serde_json::to_value(&metadata).unwrap();
        legacy_value.as_object_mut().unwrap().remove("label_kind");
        fs::write(
            backup_dir.join(METADATA_FILE),
            serde_json::to_vec_pretty(&legacy_value).unwrap(),
        )
        .unwrap();
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

        let first = create_backup(
            &config,
            &game,
            BackupCreateOptions::manual(None, Language::EnUs, false),
        )
        .unwrap();
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

        let backup = create_backup(
            &config,
            &game,
            BackupCreateOptions::manual(None, Language::EnUs, false),
        )
        .unwrap();
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

        let first = create_backup(
            &config,
            &game,
            BackupCreateOptions::manual(Some("first"), Language::EnUs, false),
        )
        .unwrap();
        assert_eq!(first.incremental_kind, Some(IncrementalBackupKind::Full));

        fs::write(save_dir.join("slot1.sav"), "two").unwrap();
        let second = create_backup(
            &config,
            &game,
            BackupCreateOptions::manual(Some("second"), Language::EnUs, false),
        )
        .unwrap();
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

    #[test]
    fn cleanup_keeps_only_latest_automatic_backup_and_preserves_protected_kinds() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot.sav"), "one").unwrap();
        let game = test_game(
            "game-1",
            "Cleanup Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let config = AppConfig {
            backup_root: root.path().join("backups"),
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let manual = create_backup(
            &config,
            &game,
            BackupCreateOptions::manual(None, Language::EnUs, false),
        )
        .unwrap();
        let custom = create_backup(
            &config,
            &game,
            BackupCreateOptions::manual(Some("boss choice"), Language::EnUs, false),
        )
        .unwrap();
        let pre_restore = create_backup(
            &config,
            &game,
            BackupCreateOptions::pre_restore(Language::EnUs),
        )
        .unwrap();

        let mut automatic_paths = Vec::new();
        for value in ["auto-one", "auto-two", "auto-three"] {
            fs::write(save_dir.join("slot.sav"), value).unwrap();
            automatic_paths.push(
                create_backup(
                    &config,
                    &game,
                    BackupCreateOptions::automatic(Language::EnUs),
                )
                .unwrap()
                .path,
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let latest_automatic = automatic_paths.last().unwrap().clone();

        assert_eq!(cleanup_old_backups(&config, &game, 1).unwrap(), 2);
        let remaining = scan_backups(&config, &game).unwrap();
        assert_eq!(
            remaining
                .iter()
                .filter(|entry| entry.label_kind == BackupLabelKind::Automatic)
                .count(),
            1
        );
        assert!(remaining.iter().any(|entry| entry.path == latest_automatic));
        assert!(remaining.iter().any(|entry| entry.path == manual.path));
        assert!(remaining.iter().any(|entry| entry.path == custom.path));
        assert!(remaining.iter().any(|entry| entry.path == pre_restore.path));
    }

    #[test]
    fn post_restore_cleanup_does_not_delete_restored_automatic_backup() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot.sav"), "old").unwrap();
        let game = test_game(
            "game-1",
            "Restore Cleanup Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let config = AppConfig {
            backup_root: root.path().join("backups"),
            games: vec![game.clone()],
            settings: Default::default(),
        };
        let restored_target = create_backup(
            &config,
            &game,
            BackupCreateOptions::automatic(Language::EnUs),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        fs::write(save_dir.join("slot.sav"), "new").unwrap();
        let newer = create_backup(
            &config,
            &game,
            BackupCreateOptions::automatic(Language::EnUs),
        )
        .unwrap();

        let mut cleanup_game = game.clone();
        cleanup_game.auto_cleanup_enabled = true;
        cleanup_game.max_backups = Some(1);
        restore_backup(&config, &cleanup_game, &restored_target).unwrap();

        let remaining = scan_backups(&config, &cleanup_game).unwrap();
        assert!(remaining
            .iter()
            .any(|entry| entry.path == restored_target.path));
        assert!(remaining.iter().any(|entry| entry.path == newer.path));
        assert_eq!(
            fs::read_to_string(save_dir.join("slot.sav")).unwrap(),
            "old"
        );
    }

    #[test]
    fn system_backup_labels_are_persisted_in_creation_language() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot.sav"), "one").unwrap();
        let game = test_game(
            "game-1",
            "Label Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let config = AppConfig {
            backup_root: root.path().join("backups"),
            games: vec![game.clone()],
            settings: Default::default(),
        };

        let zh_auto = create_backup(
            &config,
            &game,
            BackupCreateOptions::automatic(Language::ZhCn),
        )
        .unwrap();
        let zh_pre = create_backup(
            &config,
            &game,
            BackupCreateOptions::pre_restore(Language::ZhCn),
        )
        .unwrap();
        let en_auto = create_backup(
            &config,
            &game,
            BackupCreateOptions::automatic(Language::EnUs),
        )
        .unwrap();

        assert_eq!(
            read_metadata(&zh_auto.path).unwrap().label.as_deref(),
            Some("自动备份")
        );
        assert!(zh_auto
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("自动备份"));
        assert_eq!(
            read_metadata(&zh_pre.path).unwrap().label.as_deref(),
            Some("恢复前自动备份")
        );
        assert_eq!(
            read_metadata(&en_auto.path).unwrap().label.as_deref(),
            Some("Automatic backup")
        );
    }

    #[test]
    fn legacy_metadata_without_label_kind_is_inferred_and_restorable() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        let backup_root = root.path().join("backups");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot.sav"), "legacy-auto").unwrap();
        let game = test_game(
            "game-1",
            "Legacy Kind Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let config = AppConfig {
            backup_root,
            games: vec![game.clone()],
            settings: Default::default(),
        };
        let backup = create_backup(
            &config,
            &game,
            BackupCreateOptions::automatic(Language::EnUs),
        )
        .unwrap();
        let metadata_path = backup.path.join(METADATA_FILE);
        let mut value: Value = serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("label_kind");
        fs::write(&metadata_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        fs::write(save_dir.join("slot.sav"), "current").unwrap();

        let scanned = scan_backups(&config, &game).unwrap().remove(0);
        assert_eq!(scanned.label_kind, BackupLabelKind::Automatic);
        restore_backup(&config, &game, &scanned).unwrap();
        assert_eq!(
            fs::read_to_string(save_dir.join("slot.sav")).unwrap(),
            "legacy-auto"
        );
    }

    #[test]
    fn legacy_pre_restore_flag_is_inferred_without_label_kind() {
        let metadata = BackupMetadata {
            game_name: "Game".to_owned(),
            original_save_path: PathBuf::from("save"),
            created_at: Local::now().to_rfc3339(),
            label: None,
            file_count: 1,
            total_size: 1,
            tool_version: "0.1.0".to_owned(),
            is_pre_restore_backup: true,
            label_kind: BackupLabelKind::Manual,
            storage_kind: BackupStorageKind::LegacyDirectory,
            manifest_path: None,
            archive_path: None,
            snapshot_hash: None,
            stored_size: None,
            incremental_kind: None,
        };

        assert_eq!(
            infer_legacy_label_kind(&metadata),
            BackupLabelKind::PreRestore
        );
    }

    #[test]
    fn corrupted_incremental_object_stops_restore_and_keeps_current_save_and_safety_backup() {
        let root = tempdir().unwrap();
        let save_dir = root.path().join("save");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("slot.sav"), "backup-version").unwrap();
        let game = test_game(
            "game-1",
            "Corruption Game",
            &save_dir,
            BackupStorageMode::Incremental,
        );
        let config = AppConfig {
            backup_root: root.path().join("backups"),
            games: vec![game.clone()],
            settings: Default::default(),
        };
        let backup = create_backup(
            &config,
            &game,
            BackupCreateOptions::manual(Some("safe"), Language::EnUs, false),
        )
        .unwrap();
        fs::write(save_dir.join("slot.sav"), "current-version").unwrap();

        let manifest = snapshot::read_manifest(&backup.path.join(snapshot::MANIFEST_FILE)).unwrap();
        let object_hash = manifest.files[0].sha256.clone();
        let object_path = game_backup_dir(&config, &game)
            .join(snapshot::OBJECT_STORE_DIR)
            .join(&object_hash[..2])
            .join(&object_hash);
        fs::write(&object_path, "tampered").unwrap();

        let err = restore_backup(&config, &game, &backup).unwrap_err();
        assert!(matches!(err, AppError::IncrementalObjectCorrupted { .. }));
        assert_eq!(
            fs::read_to_string(save_dir.join("slot.sav")).unwrap(),
            "current-version"
        );
        let remaining = scan_backups(&config, &game).unwrap();
        assert!(remaining
            .iter()
            .any(|entry| entry.label_kind == BackupLabelKind::PreRestore));
        assert!(err
            .user_message_for_language(Language::ZhCn)
            .contains(&object_hash));
        assert!(err
            .user_message_for_language(Language::EnUs)
            .contains(&object_path.display().to_string()));
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
