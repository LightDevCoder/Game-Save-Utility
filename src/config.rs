use crate::fs_utils::{copy_dir_recursive, expand_path, remove_dir_all_if_exists};
use crate::models::{AppConfig, AppError, AppResult, AppSettings, APP_NAME};
use serde::{Deserialize, Serialize};
use serde_json::to_string_pretty;
use std::fs;
use std::path::{Path, PathBuf};

const LOCATION_FILE: &str = "GameSaveBackupTool.location.json";
const LEGACY_LOCATION_FILE: &str = "location.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppDataLocation {
    data_dir: PathBuf,
}

fn default_app_data_parent_dir() -> AppResult<PathBuf> {
    if let Some(dir) = dirs::data_dir() {
        return Ok(dir);
    }

    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::message("无法定位应用数据目录，请检查 APPDATA 环境变量"))
}

pub fn default_app_data_dir() -> AppResult<PathBuf> {
    Ok(default_app_data_parent_dir()?.join(APP_NAME))
}

pub fn app_data_dir() -> AppResult<PathBuf> {
    let default_dir = default_app_data_dir()?;
    let location_path = app_data_location_path(&default_dir)?;
    let legacy_location_path = default_dir.join(LEGACY_LOCATION_FILE);
    let location_path = if location_path.exists() {
        location_path
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

pub fn load_or_create_config() -> AppResult<AppConfig> {
    let path = config_path()?;
    if !path.exists() {
        let config = default_config()?;
        save_config(&config)?;
        return Ok(config);
    }

    let raw =
        fs::read_to_string(&path).map_err(|err| AppError::io("配置文件读取失败", &path, err))?;
    let mut config: AppConfig = serde_json::from_str(&raw)?;
    if config.backup_root.as_os_str().is_empty() || is_stale_temp_backup_root(&config.backup_root) {
        config.backup_root = default_backup_root()?;
        save_config(&config)?;
    }
    Ok(config)
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

    if backup_path.exists() {
        fs::remove_file(&backup_path)
            .map_err(|err| AppError::io("旧配置备份删除失败", &backup_path, err))?;
    }
    if path.exists() {
        fs::rename(&path, &backup_path)
            .map_err(|err| AppError::io("当前配置备份失败", &backup_path, err))?;
    }

    if let Err(err) = fs::rename(&temp_path, &path) {
        if backup_path.exists() && !path.exists() {
            let _ = fs::rename(&backup_path, &path);
        }
        return Err(AppError::io("配置文件替换失败", &path, err));
    }

    if backup_path.exists() {
        let _ = fs::remove_file(&backup_path);
    }
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
}
