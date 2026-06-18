use crate::backup;
use crate::fs_utils::expand_path;
use crate::models::{AppConfig, AppResult, GameConfig};
use crate::snapshot;
use chrono::{DateTime, Duration, Local};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupChangeState {
    NoSaveFolder,
    NoChanges,
    Changed { snapshot_hash: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackgroundBackupDecision {
    Skip,
    AutoBackup { snapshot_hash: Option<String> },
    Remind { snapshot_hash: String },
    AdvanceTimer,
}

pub fn backup_change_state(config: &AppConfig, game: &GameConfig) -> AppResult<BackupChangeState> {
    let save_path = expand_path(&game.save_path);
    let Some(current_hash) = snapshot::calculate_directory_snapshot_hash(&save_path)? else {
        return Ok(BackupChangeState::NoSaveFolder);
    };
    let latest_hash = backup::latest_snapshot_hash(config, game)?;
    if latest_hash.as_deref() == Some(current_hash.as_str()) {
        Ok(BackupChangeState::NoChanges)
    } else {
        Ok(BackupChangeState::Changed {
            snapshot_hash: current_hash,
        })
    }
}

pub fn auto_backup_is_due(game: &GameConfig, now: DateTime<Local>) -> bool {
    if !game.auto_backup.enabled {
        return false;
    }
    if let Some(next) = &game.auto_backup.next_auto_backup_at {
        return DateTime::parse_from_rfc3339(next)
            .map(|next| now >= next.with_timezone(&Local))
            .unwrap_or(true);
    }

    let interval = auto_backup_interval_duration(game);
    let Some(last) = &game.auto_backup.last_auto_backup_at else {
        return true;
    };
    let Ok(last) = DateTime::parse_from_rfc3339(last) else {
        return true;
    };
    now.signed_duration_since(last.with_timezone(&Local)) >= interval
}

pub fn next_auto_backup_time(game: &GameConfig, now: DateTime<Local>) -> DateTime<Local> {
    now + auto_backup_interval_duration(game)
}

pub fn next_auto_backup_time_string(game: &GameConfig, now: DateTime<Local>) -> Option<String> {
    if game.auto_backup.enabled {
        Some(next_auto_backup_time(game, now).to_rfc3339())
    } else {
        None
    }
}

fn auto_backup_interval_duration(game: &GameConfig) -> Duration {
    Duration::minutes(game.auto_backup.effective_interval_minutes().max(1) as i64)
}

pub fn should_show_change_reminder(
    game: &GameConfig,
    change_state: &BackupChangeState,
) -> Option<String> {
    if !game.auto_backup.change_reminder_enabled {
        return None;
    }
    let BackupChangeState::Changed { snapshot_hash } = change_state else {
        return None;
    };
    if game.auto_backup.last_reminded_snapshot_hash.as_deref() == Some(snapshot_hash.as_str()) {
        return None;
    }
    Some(snapshot_hash.clone())
}

pub fn background_backup_decision(
    game: &GameConfig,
    change_state: &BackupChangeState,
    now: DateTime<Local>,
) -> BackgroundBackupDecision {
    if !game.auto_backup.enabled && !game.auto_backup.change_reminder_enabled {
        return BackgroundBackupDecision::Skip;
    }

    if game.auto_backup.enabled && auto_backup_is_due(game, now) {
        return match change_state {
            BackupChangeState::NoSaveFolder => BackgroundBackupDecision::AdvanceTimer,
            BackupChangeState::NoChanges => BackgroundBackupDecision::AdvanceTimer,
            BackupChangeState::Changed { snapshot_hash } => BackgroundBackupDecision::AutoBackup {
                snapshot_hash: Some(snapshot_hash.clone()),
            },
        };
    }

    let BackupChangeState::Changed { snapshot_hash } = change_state else {
        return BackgroundBackupDecision::Skip;
    };

    if should_show_change_reminder(game, change_state).is_some() {
        return BackgroundBackupDecision::Remind {
            snapshot_hash: snapshot_hash.clone(),
        };
    }

    BackgroundBackupDecision::Skip
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AutoBackupConfig, BackupStorageMode};
    use chrono::TimeZone;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    #[test]
    fn auto_backup_due_respects_interval() {
        let mut game = test_game(PathBuf::from("save"));
        game.auto_backup.enabled = true;
        game.auto_backup.interval_hours = 24;
        game.auto_backup.interval_minutes = Some(24 * 60);
        game.auto_backup.last_auto_backup_at = Some(
            Local
                .with_ymd_and_hms(2026, 6, 14, 8, 0, 0)
                .unwrap()
                .to_rfc3339(),
        );

        assert!(!auto_backup_is_due(
            &game,
            Local.with_ymd_and_hms(2026, 6, 15, 7, 59, 0).unwrap()
        ));
        assert!(auto_backup_is_due(
            &game,
            Local.with_ymd_and_hms(2026, 6, 15, 8, 0, 0).unwrap()
        ));
    }

    #[test]
    fn auto_backup_due_supports_minute_intervals() {
        let mut game = test_game(PathBuf::from("save"));
        game.auto_backup.enabled = true;
        game.auto_backup.interval_hours = 1;
        game.auto_backup.interval_minutes = Some(30);
        game.auto_backup.last_auto_backup_at = Some(
            Local
                .with_ymd_and_hms(2026, 6, 15, 8, 0, 0)
                .unwrap()
                .to_rfc3339(),
        );

        assert!(!auto_backup_is_due(
            &game,
            Local.with_ymd_and_hms(2026, 6, 15, 8, 29, 0).unwrap()
        ));
        assert!(auto_backup_is_due(
            &game,
            Local.with_ymd_and_hms(2026, 6, 15, 8, 30, 0).unwrap()
        ));
    }

    #[test]
    fn background_decision_triggers_due_minute_auto_backup_only_for_changes() {
        let mut game = test_game(PathBuf::from("save"));
        game.auto_backup.enabled = true;
        game.auto_backup.interval_minutes = Some(1);
        game.auto_backup.next_auto_backup_at = Some(
            Local
                .with_ymd_and_hms(2026, 6, 15, 8, 1, 0)
                .unwrap()
                .to_rfc3339(),
        );

        let changed = BackupChangeState::Changed {
            snapshot_hash: "snapshot-a".to_owned(),
        };

        assert_eq!(
            background_backup_decision(
                &game,
                &changed,
                Local.with_ymd_and_hms(2026, 6, 15, 8, 1, 0).unwrap()
            ),
            BackgroundBackupDecision::AutoBackup {
                snapshot_hash: Some("snapshot-a".to_owned())
            }
        );
        assert_eq!(
            background_backup_decision(
                &game,
                &BackupChangeState::NoChanges,
                Local.with_ymd_and_hms(2026, 6, 15, 8, 1, 0).unwrap()
            ),
            BackgroundBackupDecision::AdvanceTimer
        );
    }

    #[test]
    fn background_decision_does_not_auto_backup_before_interval() {
        let mut game = test_game(PathBuf::from("save"));
        game.auto_backup.enabled = true;
        game.auto_backup.interval_minutes = Some(1);
        game.auto_backup.change_reminder_enabled = false;
        game.auto_backup.next_auto_backup_at = Some(
            Local
                .with_ymd_and_hms(2026, 6, 15, 8, 1, 0)
                .unwrap()
                .to_rfc3339(),
        );
        let changed = BackupChangeState::Changed {
            snapshot_hash: "snapshot-a".to_owned(),
        };

        assert_eq!(
            background_backup_decision(
                &game,
                &changed,
                Local.with_ymd_and_hms(2026, 6, 15, 8, 0, 59).unwrap()
            ),
            BackgroundBackupDecision::Skip
        );
    }

    #[test]
    fn background_decision_triggers_due_hour_auto_backup_only_for_changes() {
        let mut game = test_game(PathBuf::from("save"));
        game.auto_backup.enabled = true;
        game.auto_backup.change_reminder_enabled = false;
        game.auto_backup.interval_minutes = Some(120);
        game.auto_backup.interval_unit = crate::models::AutoBackupIntervalUnit::Hours;
        game.auto_backup.next_auto_backup_at = Some(
            Local
                .with_ymd_and_hms(2026, 6, 15, 10, 0, 0)
                .unwrap()
                .to_rfc3339(),
        );

        let changed = BackupChangeState::Changed {
            snapshot_hash: "snapshot-hour".to_owned(),
        };

        assert_eq!(
            background_backup_decision(
                &game,
                &changed,
                Local.with_ymd_and_hms(2026, 6, 15, 9, 59, 59).unwrap()
            ),
            BackgroundBackupDecision::Skip
        );
        assert_eq!(
            background_backup_decision(
                &game,
                &changed,
                Local.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap()
            ),
            BackgroundBackupDecision::AutoBackup {
                snapshot_hash: Some("snapshot-hour".to_owned())
            }
        );
        assert_eq!(
            background_backup_decision(
                &game,
                &BackupChangeState::NoChanges,
                Local.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap()
            ),
            BackgroundBackupDecision::AdvanceTimer
        );
    }

    #[test]
    fn next_auto_backup_time_supports_minutes_and_hours() {
        let now = Local.with_ymd_and_hms(2026, 6, 15, 8, 0, 0).unwrap();
        let mut game = test_game(PathBuf::from("save"));
        game.auto_backup.enabled = true;
        game.auto_backup.interval_minutes = Some(10);
        game.auto_backup.interval_hours = 1;

        assert_eq!(
            next_auto_backup_time(&game, now),
            Local.with_ymd_and_hms(2026, 6, 15, 8, 10, 0).unwrap()
        );

        game.auto_backup.interval_minutes = Some(120);
        game.auto_backup.interval_hours = 2;
        assert_eq!(
            next_auto_backup_time(&game, now),
            Local.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap()
        );
    }

    #[test]
    fn background_decision_advances_due_timer_without_changes_for_minutes_and_hours() {
        let mut game = test_game(PathBuf::from("save"));
        game.auto_backup.enabled = true;
        game.auto_backup.change_reminder_enabled = false;
        game.auto_backup.interval_minutes = Some(10);
        game.auto_backup.next_auto_backup_at = Some(
            Local
                .with_ymd_and_hms(2026, 6, 15, 8, 10, 0)
                .unwrap()
                .to_rfc3339(),
        );

        assert_eq!(
            background_backup_decision(
                &game,
                &BackupChangeState::NoChanges,
                Local.with_ymd_and_hms(2026, 6, 15, 8, 10, 0).unwrap()
            ),
            BackgroundBackupDecision::AdvanceTimer
        );

        game.auto_backup.interval_minutes = Some(120);
        game.auto_backup.next_auto_backup_at = Some(
            Local
                .with_ymd_and_hms(2026, 6, 15, 10, 0, 0)
                .unwrap()
                .to_rfc3339(),
        );
        assert_eq!(
            background_backup_decision(
                &game,
                &BackupChangeState::NoChanges,
                Local.with_ymd_and_hms(2026, 6, 15, 10, 0, 0).unwrap()
            ),
            BackgroundBackupDecision::AdvanceTimer
        );
    }

    #[test]
    fn background_decision_advances_due_timer_for_missing_save_folder() {
        let mut game = test_game(PathBuf::from("save"));
        game.auto_backup.enabled = true;
        game.auto_backup.change_reminder_enabled = false;
        game.auto_backup.interval_minutes = Some(10);
        game.auto_backup.next_auto_backup_at = Some(
            Local
                .with_ymd_and_hms(2026, 6, 15, 8, 10, 0)
                .unwrap()
                .to_rfc3339(),
        );

        assert_eq!(
            background_backup_decision(
                &game,
                &BackupChangeState::NoSaveFolder,
                Local.with_ymd_and_hms(2026, 6, 15, 8, 10, 0).unwrap()
            ),
            BackgroundBackupDecision::AdvanceTimer
        );
    }

    #[test]
    fn change_state_compares_current_save_to_latest_backup() {
        let root = tempdir().unwrap();
        let save = root.path().join("save");
        fs::create_dir_all(&save).unwrap();
        fs::write(save.join("slot.sav"), "one").unwrap();
        let game = test_game(save.clone());
        let config = AppConfig {
            backup_root: root.path().join("backups"),
            games: vec![game.clone()],
            settings: Default::default(),
        };

        assert!(matches!(
            backup_change_state(&config, &game).unwrap(),
            BackupChangeState::Changed { .. }
        ));

        backup::create_backup(
            &config,
            &game,
            backup::BackupCreateOptions::manual(None, config.settings.language, false),
        )
        .unwrap();
        assert_eq!(
            backup_change_state(&config, &game).unwrap(),
            BackupChangeState::NoChanges
        );

        fs::write(save.join("slot.sav"), "two").unwrap();
        assert!(matches!(
            backup_change_state(&config, &game).unwrap(),
            BackupChangeState::Changed { .. }
        ));
    }

    #[test]
    fn reminders_are_suppressed_for_seen_snapshot_hashes() {
        let mut game = test_game(PathBuf::from("save"));
        let state = BackupChangeState::Changed {
            snapshot_hash: "abc".to_owned(),
        };
        assert_eq!(
            should_show_change_reminder(&game, &state),
            Some("abc".to_owned())
        );

        game.auto_backup.last_reminded_snapshot_hash = Some("abc".to_owned());
        assert_eq!(should_show_change_reminder(&game, &state), None);
    }

    fn test_game(save_path: impl AsRef<Path>) -> GameConfig {
        GameConfig {
            id: "game".to_owned(),
            name: "Game".to_owned(),
            save_path: save_path.as_ref().to_path_buf(),
            max_backups: None,
            auto_cleanup_enabled: false,
            backup_storage_mode: BackupStorageMode::Incremental,
            steam_link: None,
            auto_backup: AutoBackupConfig::default(),
        }
    }
}
