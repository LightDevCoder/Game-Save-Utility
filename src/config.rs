use crate::fs_utils::{copy_dir_recursive, expand_path, remove_dir_all_if_exists};
use crate::models::{AppConfig, AppError, AppResult, AppSettings, APP_NAME};
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;
use std::fs;
use std::path::{Path, PathBuf};

const LOCATION_FILE: &str = "GameSaveUtility.location.json";
const LEGACY_APP_NAME: &str = "GameSaveBackupTool";
const LEGACY_LOCATION_FILE_BRANDED: &str = "GameSaveBackupTool.location.json";
const LEGACY_LOCATION_FILE: &str = "location.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppDataLocation {
    data_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigLoadRecovery {
    Backup,
    Default,
}

#[derive(Clone, Debug)]
pub struct ConfigLoadResult {
    pub config: AppConfig,
    pub recovery: Option<ConfigLoadRecovery>,
}

fn default_app_data_parent_dir() -> AppResult<PathBuf> {
    if let Some(dir) = dirs::data_local_dir() {
        return Ok(dir);
    }

    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::message("无法定位应用数据目录，请检查 LOCALAPPDATA 环境变量"))
}

pub fn default_app_data_dir() -> AppResult<PathBuf> {
    let parent = default_app_data_parent_dir()?;
    migrate_legacy_default_app_data_dir(&parent)
}

pub fn app_data_dir() -> AppResult<PathBuf> {
    let default_dir = default_app_data_dir()?;
    let location_path = app_data_location_path(&default_dir)?;
    let legacy_branded_location_path = default_dir
        .parent()
        .map(|parent| parent.join(LEGACY_LOCATION_FILE_BRANDED));
    let legacy_location_path = default_dir.join(LEGACY_LOCATION_FILE);
    let location_path = if location_path.exists() {
        location_path
    } else if legacy_branded_location_path
        .as_ref()
        .is_some_and(|path| path.exists())
    {
        legacy_branded_location_path.expect("checked above")
    } else if legacy_location_path.exists() {
        legacy_location_path
    } else {
        return Ok(default_dir);
    };
    if !location_path.exists() {
        return Ok(default_dir);
    }

    let raw = fs::read_to_string(&location_path)
        .map_err(|err| AppError::io("数据目录位置文件读取失败", &location_path, err))?;
    let location: AppDataLocation = serde_json::from_str(&raw)?;
    if location.data_dir.as_os_str().is_empty() {
        Ok(default_dir)
    } else {
        Ok(expand_path(&location.data_dir))
    }
}

fn migrate_legacy_default_app_data_dir(parent: &Path) -> AppResult<PathBuf> {
    let new_dir = parent.join(APP_NAME);
    let old_dir = parent.join(LEGACY_APP_NAME);
    if new_dir.exists() || !old_dir.is_dir() {
        return Ok(new_dir);
    }

    fs::rename(&old_dir, &new_dir)
        .map_err(|err| AppError::io("旧版数据目录迁移失败", &new_dir, err))?;
    Ok(new_dir)
}

pub fn default_backup_root() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("backups"))
}

pub fn config_path() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("config.json"))
}

pub fn default_config() -> AppResult<AppConfig> {
    Ok(AppConfig {
        backup_root: default_backup_root()?,
        games: Vec::new(),
        settings: AppSettings::default(),
    })
}

pub fn load_or_create_config() -> AppResult<ConfigLoadResult> {
    let path = config_path()?;
    load_or_create_config_at(&path, default_config()?)
}

fn load_or_create_config_at(path: &Path, default: AppConfig) -> AppResult<ConfigLoadResult> {
    let config_exists = path.exists();
    let backup_path = path.with_extension("json.bak");
    let backup_exists = backup_path.exists();

    if !config_exists && !backup_exists {
        save_config_at(path, &default)?;
        return Ok(ConfigLoadResult {
            config: default,
            recovery: None,
        });
    }

    if let Ok(mut config) = read_config_at(path) {
        let migrated_legacy_path = normalize_legacy_default_backup_root(path, &mut config);
        if config.backup_root.as_os_str().is_empty()
            || is_stale_temp_backup_root(&config.backup_root)
        {
            config.backup_root = default.backup_root;
            save_config_at(path, &config)?;
        } else if migrated_legacy_path {
            save_config_at(path, &config)?;
        }
        return Ok(ConfigLoadResult {
            config,
            recovery: None,
        });
    }

    if let Ok(mut config) = read_config_at(&backup_path) {
        normalize_legacy_default_backup_root(path, &mut config);
        if config.backup_root.as_os_str().is_empty()
            || is_stale_temp_backup_root(&config.backup_root)
        {
            config.backup_root = default.backup_root;
        }
        restore_config_backup(path, &config)?;
        return Ok(ConfigLoadResult {
            config,
            recovery: Some(ConfigLoadRecovery::Backup),
        });
    }

    remove_file_if_exists(path, "损坏配置文件删除失败")?;
    remove_file_if_exists(&backup_path, "损坏配置备份删除失败")?;
    save_config_at(path, &default)?;
    Ok(ConfigLoadResult {
        config: default,
        recovery: Some(ConfigLoadRecovery::Default),
    })
}

fn normalize_legacy_default_backup_root(path: &Path, config: &mut AppConfig) -> bool {
    let Some(new_dir) = path.parent() else {
        return false;
    };
    let Some(parent) = new_dir.parent() else {
        return false;
    };
    let old_dir = parent.join(LEGACY_APP_NAME);
    let original = config.backup_root.clone();
    rewrite_backup_root_for_migration(config, &old_dir, new_dir);
    config.backup_root != original
}

fn read_config_at(path: &Path) -> AppResult<AppConfig> {
    let raw =
        fs::read_to_string(path).map_err(|err| AppError::io("配置文件读取失败", path, err))?;
    Ok(serde_json::from_str(&raw)?)
}

fn restore_config_backup(path: &Path, config: &AppConfig) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| AppError::io("配置目录创建失败", parent, err))?;
    }
    let temp_path = path.with_extension("json.recovery.tmp");
    fs::write(&temp_path, to_string_pretty(config)?)
        .map_err(|err| AppError::io("配置备份恢复失败", &temp_path, err))?;
    remove_file_if_exists(path, "损坏配置文件删除失败")?;
    fs::rename(&temp_path, path).map_err(|err| AppError::io("配置备份恢复失败", path, err))
}

fn remove_file_if_exists(path: &Path, context: &str) -> AppResult<()> {
    if path.exists() {
        fs::remove_file(path).map_err(|err| AppError::io(context, path, err))?;
    }
    Ok(())
}

pub fn save_config(config: &AppConfig) -> AppResult<()> {
    let path = config_path()?;
    save_config_at(&path, config)
}

fn save_config_at(path: &Path, config: &AppConfig) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| AppError::io("配置目录创建失败", parent, err))?;
    }

    let data = to_string_pretty(config)?;
    let temp_path = path.with_extension("json.tmp");
    let backup_path = path.with_extension("json.bak");
    fs::write(&temp_path, data).map_err(|err| AppError::io("配置文件写入失败", &temp_path, err))?;

    commit_config_temp(path, &temp_path, &backup_path)
}

fn commit_config_temp(path: &Path, temp_path: &Path, backup_path: &Path) -> AppResult<()> {
    if path.exists() {
        replace_backup_with_current_config(path, backup_path)?;
    }

    if let Err(err) = fs::rename(temp_path, path) {
        if backup_path.exists() && !path.exists() {
            let _ = fs::copy(backup_path, path);
        }
        return Err(AppError::io("配置文件替换失败", path, err));
    }

    Ok(())
}

fn replace_backup_with_current_config(path: &Path, backup_path: &Path) -> AppResult<()> {
    let old_backup_path = backup_path.with_extension("bak.old");
    remove_file_if_exists(&old_backup_path, "旧配置备份删除失败")?;

    if backup_path.exists() {
        fs::rename(backup_path, &old_backup_path)
            .map_err(|err| AppError::io("旧配置备份删除失败", backup_path, err))?;
    }

    if let Err(err) = fs::rename(path, backup_path) {
        if old_backup_path.exists() && !backup_path.exists() {
            let _ = fs::rename(&old_backup_path, backup_path);
        }
        return Err(AppError::io("当前配置备份失败", backup_path, err));
    }

    let _ = fs::remove_file(old_backup_path);
    Ok(())
}

pub fn expanded_backup_root(config: &AppConfig) -> PathBuf {
    expand_path(&config.backup_root)
}

pub fn normalize_app_data_selection(path: &Path) -> PathBuf {
    if path.file_name().and_then(|name| name.to_str()) == Some(APP_NAME) {
        path.to_path_buf()
    } else {
        path.join(APP_NAME)
    }
}

pub fn migrate_app_data_dir(selected_dir: &Path, config: &mut AppConfig) -> AppResult<PathBuf> {
    let default_dir = default_app_data_dir()?;
    let old_dir = app_data_dir()?;
    let new_dir = normalize_app_data_selection(selected_dir);
    migrate_app_data_dir_inner(&old_dir, &new_dir, &default_dir, config)
}

fn migrate_app_data_dir_inner(
    old_dir: &Path,
    new_dir: &Path,
    default_dir: &Path,
    config: &mut AppConfig,
) -> AppResult<PathBuf> {
    if same_path_key(old_dir, new_dir) {
        return Ok(new_dir.to_path_buf());
    }
    if is_related_path(old_dir, new_dir) {
        return Err(AppError::message(
            "新数据目录不能放在旧数据目录内部，也不能选择旧数据目录的上级目录",
        ));
    }

    if new_dir.exists() {
        if !new_dir.is_dir() {
            return Err(AppError::message(format!(
                "新数据目录不是文件夹: {}",
                new_dir.display()
            )));
        }
        if fs::read_dir(new_dir)
            .map_err(|err| AppError::io("新数据目录读取失败", new_dir, err))?
            .next()
            .is_some()
        {
            return Err(AppError::message(format!(
                "新数据目录必须为空: {}",
                new_dir.display()
            )));
        }
    }

    let old_backup_root = expand_path(&config.backup_root);
    let backup_root_was_inside_old_data_dir =
        same_path_key(&old_backup_root, &old_dir.join("backups"))
            || old_backup_root
                .strip_prefix(old_dir)
                .map(|relative| !relative.as_os_str().is_empty())
                .unwrap_or(false);
    rewrite_backup_root_for_migration(config, old_dir, new_dir);

    if old_dir.exists() {
        let parent = new_dir
            .parent()
            .ok_or_else(|| AppError::message("新数据目录缺少父目录"))?;
        fs::create_dir_all(parent)
            .map_err(|err| AppError::io("新数据目录父目录创建失败", parent, err))?;
        let stage_dir = parent.join(format!(
            ".{}_migrate_{}",
            APP_NAME,
            uuid::Uuid::new_v4().to_string().replace('-', "")
        ));
        copy_dir_recursive(old_dir, &stage_dir)?;
        if new_dir.exists() {
            fs::remove_dir_all(new_dir)
                .map_err(|err| AppError::io("空新数据目录移除失败", new_dir, err))?;
        }
        fs::rename(&stage_dir, new_dir)
            .map_err(|err| AppError::io("新数据目录提交失败", new_dir, err))?;
    } else {
        fs::create_dir_all(new_dir)
            .map_err(|err| AppError::io("新数据目录创建失败", new_dir, err))?;
    }

    write_app_data_location(default_dir, new_dir)?;
    save_config_at(&new_dir.join("config.json"), config)?;
    cleanup_old_data_dir(old_dir)?;

    if backup_root_was_inside_old_data_dir
        && old_backup_root.exists()
        && !old_backup_root.starts_with(new_dir)
    {
        let _ = remove_dir_all_if_exists(&old_backup_root, "旧备份目录删除失败");
    }

    Ok(new_dir.to_path_buf())
}

fn write_app_data_location(default_dir: &Path, data_dir: &Path) -> AppResult<()> {
    let path = app_data_location_path(default_dir)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| AppError::io("默认数据目录创建失败", parent, err))?;
    }
    let location = AppDataLocation {
        data_dir: data_dir.to_path_buf(),
    };
    let data = to_string_pretty(&location)?;
    fs::write(&path, data).map_err(|err| AppError::io("数据目录位置文件写入失败", &path, err))
}

fn app_data_location_path(default_dir: &Path) -> AppResult<PathBuf> {
    let parent = default_dir
        .parent()
        .ok_or_else(|| AppError::message("默认数据目录缺少父目录"))?;
    Ok(parent.join(LOCATION_FILE))
}

fn cleanup_old_data_dir(old_dir: &Path) -> AppResult<()> {
    if !old_dir.exists() {
        return Ok(());
    }

    fs::remove_dir_all(old_dir).map_err(|err| AppError::io("旧数据目录删除失败", old_dir, err))
}

fn rewrite_backup_root_for_migration(config: &mut AppConfig, old_dir: &Path, new_dir: &Path) {
    let backup_root = expand_path(&config.backup_root);
    if same_path_key(&backup_root, &old_dir.join("backups")) {
        config.backup_root = new_dir.join("backups");
        return;
    }

    if let Ok(relative) = backup_root.strip_prefix(old_dir) {
        config.backup_root = new_dir.join(relative);
    }
}

fn same_path_key(left: &Path, right: &Path) -> bool {
    path_key(left) == path_key(right)
}

fn is_related_path(left: &Path, right: &Path) -> bool {
    let left = path_key(left);
    let right = path_key(right);
    left.starts_with(&(right.clone() + "\\")) || right.starts_with(&(left + "\\"))
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn is_stale_temp_backup_root(path: &Path) -> bool {
    let backup_root = expand_path(path);
    let temp_dir = std::env::temp_dir();
    let temp_key = path_key(&temp_dir);
    let backup_key = path_key(&backup_root);
    let is_in_temp_dir = backup_key.starts_with(&(temp_key + "\\"))
        || backup_key.contains("\\appdata\\local\\temp\\");
    if !is_in_temp_dir {
        return false;
    }

    backup_key.split('\\').any(|part| part.starts_with(".tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn normalize_app_data_selection_appends_app_folder_when_needed() {
        let root = PathBuf::from(r"D:\Saves");
        assert_eq!(normalize_app_data_selection(&root), root.join(APP_NAME));
        let app_root = root.join(APP_NAME);
        assert_eq!(normalize_app_data_selection(&app_root), app_root);
    }

    #[test]
    fn internal_application_identifiers_use_game_save_utility() {
        assert_eq!(APP_NAME, "GameSaveUtility");
        assert_eq!(LOCATION_FILE, "GameSaveUtility.location.json");
    }

    #[test]
    fn legacy_default_data_dir_is_migrated_when_new_dir_is_missing() {
        let root = tempdir().unwrap();
        let old_dir = root.path().join(LEGACY_APP_NAME);
        let new_dir = root.path().join(APP_NAME);
        fs::create_dir_all(old_dir.join("backups").join("Game")).unwrap();
        fs::write(old_dir.join("marker.txt"), "legacy").unwrap();
        let config = test_config(old_dir.join("backups"));
        fs::write(
            old_dir.join("config.json"),
            to_string_pretty(&config).unwrap(),
        )
        .unwrap();

        let migrated = migrate_legacy_default_app_data_dir(root.path()).unwrap();
        assert_eq!(migrated, new_dir);
        assert!(!old_dir.exists());
        assert_eq!(
            fs::read_to_string(new_dir.join("marker.txt")).unwrap(),
            "legacy"
        );

        let loaded = load_or_create_config_at(
            &new_dir.join("config.json"),
            test_config(new_dir.join("backups")),
        )
        .unwrap();
        assert_eq!(loaded.config.backup_root, new_dir.join("backups"));
        assert_eq!(
            read_config_at(&new_dir.join("config.json"))
                .unwrap()
                .backup_root,
            new_dir.join("backups")
        );
    }

    #[test]
    fn legacy_default_data_dir_is_left_untouched_when_new_dir_exists() {
        let root = tempdir().unwrap();
        let old_dir = root.path().join(LEGACY_APP_NAME);
        let new_dir = root.path().join(APP_NAME);
        fs::create_dir_all(&old_dir).unwrap();
        fs::create_dir_all(&new_dir).unwrap();
        fs::write(old_dir.join("legacy.txt"), "keep").unwrap();
        fs::write(new_dir.join("current.txt"), "current").unwrap();

        assert_eq!(
            migrate_legacy_default_app_data_dir(root.path()).unwrap(),
            new_dir
        );
        assert!(old_dir.join("legacy.txt").exists());
        assert!(new_dir.join("current.txt").exists());
    }

    #[test]
    fn migrate_app_data_dir_moves_files_and_rewrites_default_backup_root() {
        let temp = tempdir().unwrap();
        let old_dir = temp.path().join("old").join(APP_NAME);
        let default_dir = old_dir.clone();
        let selected_dir = temp.path().join("new_parent");
        let new_dir = selected_dir.join(APP_NAME);
        fs::create_dir_all(old_dir.join("backups").join("Game")).unwrap();
        fs::write(old_dir.join("config.json"), "{}").unwrap();
        fs::write(
            old_dir.join("backups").join("Game").join("slot.sav"),
            "save",
        )
        .unwrap();

        let mut config = AppConfig {
            backup_root: old_dir.join("backups"),
            games: Vec::new(),
            settings: AppSettings::default(),
        };

        let migrated =
            migrate_app_data_dir_inner(&old_dir, &new_dir, &default_dir, &mut config).unwrap();

        assert_eq!(migrated, new_dir);
        assert!(new_dir
            .join("backups")
            .join("Game")
            .join("slot.sav")
            .exists());
        assert!(default_dir.parent().unwrap().join(LOCATION_FILE).exists());
        assert!(!default_dir.exists());
        assert_eq!(config.backup_root, new_dir.join("backups"));
    }

    #[test]
    fn stale_temp_backup_root_is_detected() {
        let stale_root = std::env::temp_dir().join(".tmpFGD30x").join("backups");
        assert!(is_stale_temp_backup_root(&stale_root));

        let stable_root = PathBuf::from(r"D:\Temp\GSU-Test\Backups");
        assert!(!is_stale_temp_backup_root(&stable_root));
    }

    #[test]
    fn migrate_app_data_dir_preserves_custom_backup_root_outside_data_dir() {
        let temp = tempdir().unwrap();
        let old_dir = temp.path().join("old").join(APP_NAME);
        let default_dir = old_dir.clone();
        let new_dir = temp.path().join("new_parent").join(APP_NAME);
        let custom_backup_root = temp.path().join("custom_backups");
        fs::create_dir_all(&old_dir).unwrap();
        fs::create_dir_all(&custom_backup_root).unwrap();
        fs::write(custom_backup_root.join("keep.sav"), "save").unwrap();
        fs::write(old_dir.join("config.json"), "{}").unwrap();

        let mut config = AppConfig {
            backup_root: custom_backup_root.clone(),
            games: Vec::new(),
            settings: AppSettings::default(),
        };

        migrate_app_data_dir_inner(&old_dir, &new_dir, &default_dir, &mut config).unwrap();

        assert_eq!(config.backup_root, custom_backup_root);
        assert!(config.backup_root.join("keep.sav").exists());
    }

    #[test]
    fn missing_config_is_restored_from_backup_and_rewritten() {
        let root = tempdir().unwrap();
        let path = root.path().join("config.json");
        let backup_path = path.with_extension("json.bak");
        let expected = test_config(PathBuf::from(r"D:\RecoveredBackups"));
        fs::write(&backup_path, to_string_pretty(&expected).unwrap()).unwrap();

        let result =
            load_or_create_config_at(&path, test_config(PathBuf::from(r"D:\DefaultBackups")))
                .unwrap();

        assert_eq!(result.recovery, Some(ConfigLoadRecovery::Backup));
        assert_eq!(result.config.backup_root, expected.backup_root);
        assert!(path.exists());
        assert_eq!(
            read_config_at(&path).unwrap().backup_root,
            expected.backup_root
        );
    }

    #[test]
    fn first_launch_without_config_or_backup_creates_default_without_recovery_warning() {
        let root = tempdir().unwrap();
        let path = root.path().join("config.json");
        let default = test_config(PathBuf::from(r"D:\DefaultBackups"));

        let result = load_or_create_config_at(&path, default.clone()).unwrap();

        assert_eq!(result.recovery, None);
        assert_eq!(result.config.backup_root, default.backup_root);
        assert_eq!(
            read_config_at(&path).unwrap().backup_root,
            default.backup_root
        );
        assert!(!path.with_extension("json.bak").exists());
    }

    #[test]
    fn corrupted_config_is_restored_from_valid_backup() {
        let root = tempdir().unwrap();
        let path = root.path().join("config.json");
        let backup_path = path.with_extension("json.bak");
        let expected = test_config(PathBuf::from(r"D:\RecoveredBackups"));
        fs::write(&path, "{broken").unwrap();
        fs::write(&backup_path, to_string_pretty(&expected).unwrap()).unwrap();

        let result =
            load_or_create_config_at(&path, test_config(PathBuf::from(r"D:\DefaultBackups")))
                .unwrap();

        assert_eq!(result.recovery, Some(ConfigLoadRecovery::Backup));
        assert_eq!(
            read_config_at(&path).unwrap().backup_root,
            expected.backup_root
        );
    }

    #[test]
    fn recovered_backup_rewrites_normalized_backup_root() {
        let root = tempdir().unwrap();
        let path = root.path().join("config.json");
        let backup_path = path.with_extension("json.bak");
        let stale = test_config(std::env::temp_dir().join(".tmp-old").join("backups"));
        let default = test_config(PathBuf::from(r"D:\DefaultBackups"));
        fs::write(&backup_path, to_string_pretty(&stale).unwrap()).unwrap();

        let result = load_or_create_config_at(&path, default.clone()).unwrap();

        assert_eq!(result.recovery, Some(ConfigLoadRecovery::Backup));
        assert_eq!(result.config.backup_root, default.backup_root);
        assert_eq!(
            read_config_at(&path).unwrap().backup_root,
            default.backup_root
        );
    }

    #[test]
    fn corrupted_config_and_backup_fall_back_to_default() {
        let root = tempdir().unwrap();
        let path = root.path().join("config.json");
        let backup_path = path.with_extension("json.bak");
        let default = test_config(root.path().join("default"));
        fs::write(&path, "{broken").unwrap();
        fs::write(&backup_path, "{also-broken").unwrap();

        let result = load_or_create_config_at(&path, default.clone()).unwrap();

        assert_eq!(result.recovery, Some(ConfigLoadRecovery::Default));
        assert_eq!(result.config.backup_root, default.backup_root);
        assert_eq!(
            read_config_at(&path).unwrap().backup_root,
            default.backup_root
        );
    }

    #[test]
    fn save_config_preserves_existing_backup_when_config_file_is_missing() {
        let root = tempdir().unwrap();
        let path = root.path().join("config.json");
        let backup_path = path.with_extension("json.bak");
        let backup = test_config(PathBuf::from(r"D:\BackupCopy"));
        let new_config = test_config(PathBuf::from(r"D:\NewConfig"));
        fs::write(&backup_path, to_string_pretty(&backup).unwrap()).unwrap();

        save_config_at(&path, &new_config).unwrap();

        assert_eq!(
            read_config_at(&path).unwrap().backup_root,
            new_config.backup_root
        );
        assert_eq!(
            read_config_at(&backup_path).unwrap().backup_root,
            backup.backup_root
        );
    }

    #[test]
    fn save_config_replaces_backup_with_previous_config_when_config_exists() {
        let root = tempdir().unwrap();
        let path = root.path().join("config.json");
        let backup_path = path.with_extension("json.bak");
        let old_backup = test_config(PathBuf::from(r"D:\OldBackup"));
        let previous = test_config(PathBuf::from(r"D:\PreviousConfig"));
        let new_config = test_config(PathBuf::from(r"D:\NewConfig"));
        fs::write(&path, to_string_pretty(&previous).unwrap()).unwrap();
        fs::write(&backup_path, to_string_pretty(&old_backup).unwrap()).unwrap();

        save_config_at(&path, &new_config).unwrap();

        assert_eq!(
            read_config_at(&path).unwrap().backup_root,
            new_config.backup_root
        );
        assert_eq!(
            read_config_at(&backup_path).unwrap().backup_root,
            previous.backup_root
        );
    }

    #[test]
    fn save_config_restores_previous_config_if_commit_fails() {
        let root = tempdir().unwrap();
        let path = root.path().join("config.json");
        let temp_path = path.with_extension("missing.tmp");
        let backup_path = path.with_extension("json.bak");
        let old_backup = test_config(PathBuf::from(r"D:\OldBackup"));
        let previous = test_config(PathBuf::from(r"D:\PreviousConfig"));
        fs::write(&path, to_string_pretty(&previous).unwrap()).unwrap();
        fs::write(&backup_path, to_string_pretty(&old_backup).unwrap()).unwrap();

        assert!(commit_config_temp(&path, &temp_path, &backup_path).is_err());

        assert_eq!(
            read_config_at(&path).unwrap().backup_root,
            previous.backup_root
        );
        assert_eq!(
            read_config_at(&backup_path).unwrap().backup_root,
            previous.backup_root
        );
    }

    fn test_config(backup_root: PathBuf) -> AppConfig {
        AppConfig {
            backup_root,
            games: Vec::new(),
            settings: AppSettings::default(),
        }
    }
}
