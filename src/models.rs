use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

pub const APP_NAME: &str = "GameSaveUtility";
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAIN_WINDOW_DEFAULT_WIDTH: f32 = 1100.0;
pub const MAIN_WINDOW_DEFAULT_HEIGHT: f32 = 760.0;
pub const MAIN_WINDOW_MIN_WIDTH: f32 = 1024.0;
pub const MAIN_WINDOW_MIN_HEIGHT: f32 = 680.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub backup_root: PathBuf,
    pub games: Vec<GameConfig>,
    #[serde(default)]
    pub settings: AppSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameConfig {
    pub id: String,
    pub name: String,
    pub save_path: PathBuf,
    pub max_backups: Option<usize>,
    pub auto_cleanup_enabled: bool,
    #[serde(default)]
    pub backup_storage_mode: BackupStorageMode,
    #[serde(default)]
    pub steam_link: Option<SteamLink>,
    #[serde(default)]
    pub auto_backup: AutoBackupConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_close_behavior")]
    pub close_behavior: CloseBehavior,
    #[serde(default)]
    pub close_behavior_prompted: bool,
    #[serde(default)]
    pub main_window: WindowSettings,
    #[serde(default)]
    pub language: Language,
    #[serde(default = "default_keyboard_shortcuts_enabled")]
    pub keyboard_shortcuts_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            close_behavior: default_close_behavior(),
            close_behavior_prompted: false,
            main_window: WindowSettings::default(),
            language: Language::default(),
            keyboard_shortcuts_enabled: default_keyboard_shortcuts_enabled(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseBehavior {
    MinimizeToTray,
    Exit,
}

fn default_close_behavior() -> CloseBehavior {
    CloseBehavior::MinimizeToTray
}

fn default_keyboard_shortcuts_enabled() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    ZhCn,
    EnUs,
}

impl Language {
    pub fn toggled(self) -> Self {
        match self {
            Self::ZhCn => Self::EnUs,
            Self::EnUs => Self::ZhCn,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupLabelKind {
    #[default]
    Manual,
    Automatic,
    PreRestore,
}

pub fn system_backup_label(kind: BackupLabelKind, language: Language) -> Option<&'static str> {
    match (kind, language) {
        (BackupLabelKind::Manual, _) => None,
        (BackupLabelKind::Automatic, Language::ZhCn) => Some("自动备份"),
        (BackupLabelKind::Automatic, Language::EnUs) => Some("Automatic backup"),
        (BackupLabelKind::PreRestore, Language::ZhCn) => Some("恢复前自动备份"),
        (BackupLabelKind::PreRestore, Language::EnUs) => Some("Pre-restore automatic backup"),
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct WindowSettings {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub maximized: bool,
}

#[derive(Clone, Debug)]
pub struct BackupEntry {
    pub game_id: String,
    pub game_name: String,
    pub path: PathBuf,
    pub created_at: DateTime<Local>,
    pub label: Option<String>,
    pub file_count: u64,
    pub total_size: u64,
    pub stored_size: Option<u64>,
    pub is_pre_restore_backup: bool,
    pub label_kind: BackupLabelKind,
    pub storage_kind: BackupStorageKind,
    pub incremental_kind: Option<IncrementalBackupKind>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupMetadata {
    pub game_name: String,
    pub original_save_path: PathBuf,
    pub created_at: String,
    pub label: Option<String>,
    pub file_count: u64,
    pub total_size: u64,
    pub tool_version: String,
    pub is_pre_restore_backup: bool,
    #[serde(default)]
    pub label_kind: BackupLabelKind,
    #[serde(default)]
    pub storage_kind: BackupStorageKind,
    #[serde(default)]
    pub manifest_path: Option<PathBuf>,
    #[serde(default)]
    pub archive_path: Option<PathBuf>,
    #[serde(default)]
    pub snapshot_hash: Option<String>,
    #[serde(default)]
    pub stored_size: Option<u64>,
    #[serde(default)]
    pub incremental_kind: Option<IncrementalBackupKind>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupStorageMode {
    #[default]
    Incremental,
    Zip,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupStorageKind {
    #[default]
    LegacyDirectory,
    Incremental,
    Zip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncrementalBackupKind {
    Full,
    Incremental,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SteamLink {
    pub app_id: String,
    #[serde(default)]
    pub cloud_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoBackupConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_auto_backup_interval_hours")]
    pub interval_hours: u64,
    #[serde(default)]
    pub interval_minutes: Option<u64>,
    #[serde(default)]
    pub interval_unit: AutoBackupIntervalUnit,
    #[serde(default = "default_change_reminder_enabled")]
    pub change_reminder_enabled: bool,
    #[serde(default)]
    pub last_auto_backup_at: Option<String>,
    #[serde(default)]
    pub last_reminded_snapshot_hash: Option<String>,
    #[serde(default)]
    pub next_auto_backup_at: Option<String>,
}

impl Default for AutoBackupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: default_auto_backup_interval_hours(),
            interval_minutes: Some(default_auto_backup_interval_minutes()),
            interval_unit: AutoBackupIntervalUnit::Hours,
            change_reminder_enabled: default_change_reminder_enabled(),
            last_auto_backup_at: None,
            last_reminded_snapshot_hash: None,
            next_auto_backup_at: None,
        }
    }
}

impl AutoBackupConfig {
    pub fn effective_interval_minutes(&self) -> u64 {
        self.interval_minutes
            .unwrap_or_else(|| self.interval_hours.saturating_mul(60))
            .max(1)
    }

    pub fn display_interval_unit(&self) -> AutoBackupIntervalUnit {
        let minutes = self.effective_interval_minutes();
        match self.interval_unit {
            AutoBackupIntervalUnit::Hours if minutes.is_multiple_of(60) => {
                AutoBackupIntervalUnit::Hours
            }
            AutoBackupIntervalUnit::Hours => AutoBackupIntervalUnit::Minutes,
            AutoBackupIntervalUnit::Minutes => AutoBackupIntervalUnit::Minutes,
        }
    }

    pub fn display_interval_value(&self) -> u64 {
        let minutes = self.effective_interval_minutes();
        match self.display_interval_unit() {
            AutoBackupIntervalUnit::Hours => (minutes / 60).max(1),
            AutoBackupIntervalUnit::Minutes => minutes,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoBackupIntervalUnit {
    Minutes,
    #[default]
    Hours,
}

fn default_auto_backup_interval_hours() -> u64 {
    24
}

fn default_auto_backup_interval_minutes() -> u64 {
    default_auto_backup_interval_hours() * 60
}

fn default_change_reminder_enabled() -> bool {
    true
}

#[derive(Clone, Debug)]
pub struct PresetGame {
    pub name: String,
    pub path_candidates: Vec<String>,
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct StatusMessage {
    pub kind: StatusKind,
    pub text: String,
}

impl StatusMessage {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Info,
            text: text.into(),
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Success,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Warning,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Error,
            text: text.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    Message { message: String },

    #[error("{context}: {path:?} ({source})")]
    Io {
        context: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("配置或元数据 JSON 格式错误: {0}")]
    Json(#[from] serde_json::Error),

    #[error("存档目录为空: {path:?}")]
    EmptySaveDir { path: PathBuf },

    #[error("Incremental backup object is corrupted: {sha256} at {path:?}")]
    IncrementalObjectCorrupted { sha256: String, path: PathBuf },
}

impl AppError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message {
            message: message.into(),
        }
    }

    pub fn io(
        context: impl Into<String>,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            context: context.into(),
            path: path.into(),
            source,
        }
    }

    pub fn user_message_for_language(&self, language: Language) -> String {
        match self {
            Self::Message { message } => localize_error_message(message, language),
            Self::Io { context, path, .. } => {
                format!(
                    "{}: {}",
                    localize_error_context(context, language),
                    path.display()
                )
            }
            Self::Json(_) => match language {
                Language::ZhCn => "配置或元数据 JSON 格式错误".to_owned(),
                Language::EnUs => "Configuration or metadata JSON is invalid".to_owned(),
            },
            Self::EmptySaveDir { path } => match language {
                Language::ZhCn => format!("存档目录为空: {}", path.display()),
                Language::EnUs => format!("The save folder is empty: {}", path.display()),
            },
            Self::IncrementalObjectCorrupted { sha256, path } => match language {
                Language::ZhCn => format!(
                    "增量备份对象已损坏，恢复已停止（对象 {sha256}，路径 {}）",
                    path.display()
                ),
                Language::EnUs => format!(
                    "Incremental backup object is corrupted; restore has been stopped (object {sha256}, path {})",
                    path.display()
                ),
            },
        }
    }
}

fn localize_error_context(context: &str, language: Language) -> String {
    let pair = match context {
        "Backup folder create failed" => ("备份目录创建失败", "Backup folder creation failed"),
        "Backup temp folder create failed" => (
            "备份临时目录创建失败",
            "Backup temporary folder creation failed",
        ),
        "Backup folder commit failed" => ("备份目录提交失败", "Backup folder commit failed"),
        "Backup folder read failed" => ("备份目录读取失败", "Backup folder read failed"),
        "Backup folder entry read failed" => {
            ("备份目录项读取失败", "Backup folder entry read failed")
        }
        "Backup folder entry type read failed" => (
            "备份目录项类型读取失败",
            "Backup folder entry type read failed",
        ),
        "Backup delete failed" => ("备份删除失败", "Backup deletion failed"),
        "Backup metadata write failed" => ("备份元数据写入失败", "Backup metadata write failed"),
        "Backup metadata read failed" => ("备份元数据读取失败", "Backup metadata read failed"),
        "Current save folder move failed" => {
            ("当前存档目录移动失败", "Current save folder move failed")
        }
        "Target save folder create failed" => {
            ("目标存档目录创建失败", "Target save folder creation failed")
        }
        "Target save parent folder create failed" => (
            "目标存档父目录创建失败",
            "Target save parent folder creation failed",
        ),
        "Game backup folder read failed" => {
            ("游戏备份目录读取失败", "Game backup folder read failed")
        }
        "Game backup entry read failed" => {
            ("游戏备份目录项读取失败", "Game backup entry read failed")
        }
        "Incremental manifest read failed" => (
            "增量备份清单读取失败",
            "Incremental backup manifest read failed",
        ),
        "Incremental manifest write failed" => (
            "增量备份清单写入失败",
            "Incremental backup manifest write failed",
        ),
        "Incremental object write failed" => (
            "增量备份对象写入失败",
            "Incremental backup object write failed",
        ),
        "Incremental object commit failed" => (
            "增量备份对象提交失败",
            "Incremental backup object commit failed",
        ),
        "Hash file open failed" => ("哈希文件打开失败", "Hash file open failed"),
        "Hash file read failed" => ("哈希文件读取失败", "Hash file read failed"),
        "Unreferenced object delete failed" => (
            "未引用增量对象删除失败",
            "Unreferenced object deletion failed",
        ),
        "ZIP backup parent create failed" => (
            "ZIP 备份父目录创建失败",
            "ZIP backup parent folder creation failed",
        ),
        "ZIP backup file create failed" => {
            ("ZIP 备份文件创建失败", "ZIP backup file creation failed")
        }
        "ZIP backup file open failed" => ("ZIP 备份文件打开失败", "ZIP backup file open failed"),
        "ZIP source file open failed" => ("ZIP 源文件打开失败", "ZIP source file open failed"),
        "ZIP file write failed" => ("ZIP 文件写入失败", "ZIP file write failed"),
        "ZIP metadata read failed" => ("ZIP 文件信息读取失败", "ZIP metadata read failed"),
        "ZIP extract target create failed" => (
            "ZIP 解压目标目录创建失败",
            "ZIP extraction target creation failed",
        ),
        "ZIP directory extract failed" => ("ZIP 目录解压失败", "ZIP directory extraction failed"),
        "ZIP extract parent create failed" => (
            "ZIP 解压父目录创建失败",
            "ZIP extraction parent folder creation failed",
        ),
        "ZIP output file create failed" => {
            ("ZIP 输出文件创建失败", "ZIP output file creation failed")
        }
        "ZIP output file write failed" => ("ZIP 输出文件写入失败", "ZIP output file write failed"),
        "配置文件读取失败" => ("配置文件读取失败", "Configuration file read failed"),
        "配置文件写入失败" => ("配置文件写入失败", "Configuration file write failed"),
        "配置文件替换失败" => ("配置文件替换失败", "Configuration file replacement failed"),
        "配置目录创建失败" => ("配置目录创建失败", "Configuration folder creation failed"),
        "配置备份恢复失败" => (
            "配置备份恢复失败",
            "Configuration backup restoration failed",
        ),
        "旧配置备份删除失败" => (
            "旧配置备份处理失败",
            "Previous configuration backup handling failed",
        ),
        "当前配置备份失败" => ("当前配置备份失败", "Current configuration backup failed"),
        "数据目录位置文件读取失败" => (
            "数据目录位置文件读取失败",
            "Data-folder location file read failed",
        ),
        "数据目录位置文件写入失败" => (
            "数据目录位置文件写入失败",
            "Data-folder location file write failed",
        ),
        "新数据目录读取失败" => ("新数据目录读取失败", "New data folder read failed"),
        "新数据目录父目录创建失败" => (
            "新数据目录父目录创建失败",
            "New data folder parent creation failed",
        ),
        "空新数据目录移除失败" => (
            "空的新数据目录移除失败",
            "Empty new data folder removal failed",
        ),
        "新数据目录提交失败" => ("新数据目录提交失败", "New data folder commit failed"),
        "新数据目录创建失败" => ("新数据目录创建失败", "New data folder creation failed"),
        "默认数据目录创建失败" => (
            "默认数据目录创建失败",
            "Default data folder creation failed",
        ),
        "旧数据目录删除失败" => ("旧数据目录删除失败", "Old data folder deletion failed"),
        "Steam 清单读取失败" => ("Steam 清单读取失败", "Steam manifest read failed"),
        "日志目录创建失败" => ("日志目录创建失败", "Log folder creation failed"),
        "目录读取失败" => ("目录读取失败", "Folder read failed"),
        "目录项读取失败" => ("目录项读取失败", "Folder entry read failed"),
        "文件类型读取失败" => ("文件类型读取失败", "File type read failed"),
        "文件信息读取失败" => ("文件信息读取失败", "File metadata read failed"),
        "目录创建失败" => ("目录创建失败", "Folder creation failed"),
        "源目录读取失败" => ("源目录读取失败", "Source folder read failed"),
        "源目录项读取失败" => ("源目录项读取失败", "Source folder entry read failed"),
        "路径规范化失败" => ("路径规范化失败", "Path normalization failed"),
        "没有读取权限或目录不可访问" => (
            "没有读取权限或目录不可访问",
            "The folder is inaccessible or cannot be read",
        ),
        _ => return localize_error_message(context, language),
    };
    match language {
        Language::ZhCn => pair.0.to_owned(),
        Language::EnUs => pair.1.to_owned(),
    }
}

fn localize_error_message(message: &str, language: Language) -> String {
    let pair = if message.starts_with("Backup path is not a folder") {
        ("备份路径不是文件夹", "Backup path is not a folder")
    } else if message.starts_with("Backup root cannot be inside") {
        (
            "备份根目录不能位于游戏存档目录内",
            "The backup root cannot be inside the game save folder",
        )
    } else if message.starts_with("Backup node does not exist") {
        ("备份节点不存在", "The backup node does not exist")
    } else if message.starts_with("Target save path is not a folder") {
        (
            "目标存档路径不是文件夹",
            "The target save path is not a folder",
        )
    } else if message.starts_with("Target save path has no parent folder") {
        (
            "目标存档路径没有父目录",
            "The target save path has no parent folder",
        )
    } else if message.starts_with("Incremental object is missing") {
        (
            "增量备份对象缺失",
            "An incremental backup object is missing",
        )
    } else if message.starts_with("Incremental backup manifest does not exist") {
        (
            "增量备份清单不存在",
            "The incremental backup manifest does not exist",
        )
    } else if message.starts_with("Manifest contains") {
        (
            "增量备份清单包含无效数据",
            "The incremental backup manifest contains invalid data",
        )
    } else if message.starts_with("ZIP") {
        ("ZIP 备份处理失败", "ZIP backup processing failed")
    } else if message.starts_with("Save walk failed")
        || message.starts_with("Relative path calculation failed")
        || message.starts_with("Save path is not a folder")
        || message.starts_with("Snapshot hash does not support")
    {
        ("存档目录扫描失败", "Save folder scan failed")
    } else if message.starts_with("无法定位应用数据目录") {
        (
            "无法定位应用数据目录，请检查 LOCALAPPDATA 环境变量",
            "The application data folder could not be located; check the LOCALAPPDATA environment variable",
        )
    } else if message.starts_with("未找到 Steam 库目录") {
        ("未找到 Steam 库目录", "No Steam library folder was found")
    } else {
        return match language {
            Language::ZhCn
                if message
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_alphabetic()) =>
            {
                "操作失败".to_owned()
            }
            Language::EnUs
                if message
                    .chars()
                    .next()
                    .is_some_and(|character| ('\u{4e00}'..='\u{9fff}').contains(&character)) =>
            {
                "Operation failed".to_owned()
            }
            _ => message.to_owned(),
        };
    };
    match language {
        Language::ZhCn => pair.0.to_owned(),
        Language::EnUs => pair.1.to_owned(),
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn backup_commit_error_is_localized_without_english_context_in_chinese() {
        let err = AppError::io(
            "Backup folder commit failed",
            PathBuf::from("backup"),
            std::io::Error::other("detail"),
        );
        let message = err.user_message_for_language(Language::ZhCn);
        assert!(message.contains("备份目录提交失败"));
        assert!(!message.contains("Backup folder commit failed"));
    }

    #[test]
    fn config_read_error_is_localized_without_chinese_context_in_english() {
        let err = AppError::io(
            "配置文件读取失败",
            PathBuf::from("config.json"),
            std::io::Error::other("detail"),
        );
        let message = err.user_message_for_language(Language::EnUs);
        assert!(message.contains("Configuration file read failed"));
        assert!(!message.contains("配置文件读取失败"));
    }

    #[test]
    fn empty_save_dir_and_json_errors_are_localized() {
        let empty = AppError::EmptySaveDir {
            path: PathBuf::from("save"),
        };
        assert!(empty
            .user_message_for_language(Language::ZhCn)
            .contains("存档目录为空"));
        assert!(empty
            .user_message_for_language(Language::EnUs)
            .contains("save folder is empty"));

        let json: AppError = serde_json::from_str::<serde_json::Value>("{")
            .unwrap_err()
            .into();
        assert_eq!(
            json.user_message_for_language(Language::ZhCn),
            "配置或元数据 JSON 格式错误"
        );
        assert_eq!(
            json.user_message_for_language(Language::EnUs),
            "Configuration or metadata JSON is invalid"
        );
    }

    #[test]
    fn restore_move_failure_context_localizes_in_chinese_and_english() {
        assert_context_localizes(
            "Current save folder move failed",
            "当前存档目录移动失败",
            "Current save folder move failed",
        );
    }

    #[test]
    fn hash_file_read_failure_context_localizes_in_chinese_and_english() {
        assert_context_localizes(
            "Hash file read failed",
            "哈希文件读取失败",
            "Hash file read failed",
        );
    }

    #[test]
    fn config_backup_restore_failure_context_localizes_in_chinese_and_english() {
        assert_context_localizes(
            "配置备份恢复失败",
            "配置备份恢复失败",
            "Configuration backup restoration failed",
        );
    }

    #[test]
    fn zip_extraction_failure_context_localizes_in_chinese_and_english() {
        assert_context_localizes(
            "ZIP output file write failed",
            "ZIP 输出文件写入失败",
            "ZIP output file write failed",
        );
    }

    fn assert_context_localizes(context: &str, chinese: &str, english: &str) {
        let err = AppError::io(
            context,
            PathBuf::from("path"),
            std::io::Error::other("detail"),
        );
        let zh = err.user_message_for_language(Language::ZhCn);
        let en = err.user_message_for_language(Language::EnUs);
        assert!(zh.contains(chinese));
        assert!(!zh.contains(english), "Chinese message was mixed: {zh}");
        assert!(en.contains(english));
        if chinese != english {
            assert!(!en.contains(chinese), "English message was mixed: {en}");
        }
    }
}
