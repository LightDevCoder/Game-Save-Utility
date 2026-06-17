use crate::fs_utils::expand_path;
use crate::models::{AppError, AppResult, GameConfig};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

const CLOUD_CONFLICT_THRESHOLD: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudConflict {
    pub local_path: PathBuf,
    pub cloud_path: PathBuf,
    pub local_latest: SystemTime,
    pub cloud_latest: SystemTime,
}

pub fn check_steam_cloud_conflict(game: &GameConfig) -> AppResult<Option<CloudConflict>> {
    check_steam_cloud_conflict_with_threshold(game, CLOUD_CONFLICT_THRESHOLD)
}

fn check_steam_cloud_conflict_with_threshold(
    game: &GameConfig,
    threshold: Duration,
) -> AppResult<Option<CloudConflict>> {
    let Some(link) = &game.steam_link else {
        return Ok(None);
    };

    let local_path = expand_path(&game.save_path);
    let Some(local_latest) = latest_modified(&local_path)? else {
        return Ok(None);
    };

    for cloud_path in &link.cloud_paths {
        let cloud_path = expand_path(cloud_path);
        if same_path_key(&local_path, &cloud_path) {
            continue;
        }
        let Some(cloud_latest) = latest_modified(&cloud_path)? else {
            continue;
        };
        if times_differ(local_latest, cloud_latest, threshold) {
            return Ok(Some(CloudConflict {
                local_path: local_path.clone(),
                cloud_path,
                local_latest,
                cloud_latest,
            }));
        }
    }

    Ok(None)
}

fn latest_modified(path: &Path) -> AppResult<Option<SystemTime>> {
    if !path.exists() {
        return Ok(None);
    }
    if path.is_file() {
        return Ok(Some(
            fs::metadata(path)
                .map_err(|err| AppError::io("File metadata read failed", path, err))?
                .modified()
                .map_err(|err| AppError::io("File modified time read failed", path, err))?,
        ));
    }
    if !path.is_dir() {
        return Ok(None);
    }

    let mut latest = None;
    for entry in WalkDir::new(path).follow_links(false) {
        let entry =
            entry.map_err(|err| AppError::message(format!("Cloud path scan failed: {err}")))?;
        if entry.file_type().is_symlink() || !entry.file_type().is_file() {
            continue;
        }
        let modified = fs::metadata(entry.path())
            .map_err(|err| AppError::io("Cloud file metadata read failed", entry.path(), err))?
            .modified()
            .map_err(|err| {
                AppError::io("Cloud file modified time read failed", entry.path(), err)
            })?;
        latest = Some(match latest {
            Some(current) if current >= modified => current,
            _ => modified,
        });
    }
    Ok(latest)
}

fn times_differ(left: SystemTime, right: SystemTime, threshold: Duration) -> bool {
    left.duration_since(right)
        .or_else(|_| right.duration_since(left))
        .map(|diff| diff > threshold)
        .unwrap_or(false)
}

fn same_path_key(left: &Path, right: &Path) -> bool {
    left.to_string_lossy().replace('/', "\\").to_lowercase()
        == right.to_string_lossy().replace('/', "\\").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AutoBackupConfig, BackupStorageMode, SteamLink};
    use tempfile::tempdir;

    #[test]
    fn detects_local_cloud_time_difference() {
        let root = tempdir().unwrap();
        let local = root.path().join("local");
        let cloud = root.path().join("cloud");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&cloud).unwrap();
        fs::write(local.join("slot.sav"), "local").unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        fs::write(cloud.join("slot.sav"), "cloud").unwrap();

        let game = game_with_cloud(&local, vec![cloud]);
        let conflict =
            check_steam_cloud_conflict_with_threshold(&game, Duration::from_millis(1)).unwrap();

        assert!(conflict.is_some());
    }

    #[test]
    fn skips_cloud_path_when_it_is_same_as_local_path() {
        let root = tempdir().unwrap();
        let local = root.path().join("local");
        fs::create_dir_all(&local).unwrap();
        fs::write(local.join("slot.sav"), "local").unwrap();

        let game = game_with_cloud(&local, vec![local.clone()]);
        assert!(check_steam_cloud_conflict(&game).unwrap().is_none());
    }

    fn game_with_cloud(local: &Path, cloud_paths: Vec<PathBuf>) -> GameConfig {
        GameConfig {
            id: "game".to_owned(),
            name: "Game".to_owned(),
            save_path: local.to_path_buf(),
            max_backups: None,
            auto_cleanup_enabled: false,
            backup_storage_mode: BackupStorageMode::Incremental,
            steam_link: Some(SteamLink {
                app_id: "1".to_owned(),
                cloud_paths,
            }),
            auto_backup: AutoBackupConfig::default(),
        }
    }
}
