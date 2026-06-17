use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

pub const APP_NAME: &str = "GameSaveBackupTool";
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
            AutoBackupIntervalUnit::Hours if minutes % 60 == 0 => AutoBackupIntervalUnit::Hours,
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

    pub fn user_message(&self) -> String {
        match self {
            Self::Message { message } => message.clone(),
            Self::Io {
                context,
                path,
                source,
            } => format!("{context}: {} ({source})", path.display()),
            Self::Json(err) => format!("配置或元数据 JSON 格式错误: {err}"),
            Self::EmptySaveDir { path } => format!("存档目录为空: {}", path.display()),
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
