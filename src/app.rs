use crate::config;
use crate::fs_utils::format_size;
use crate::i18n::{t, Text as T};
use crate::logger;
use crate::models::{
    AppConfig, AppError, AppResult, AutoBackupConfig, AutoBackupIntervalUnit, BackupEntry,
    BackupStorageMode, CloseBehavior, GameConfig, Language, PresetGame, StatusKind, StatusMessage,
    SteamLink, MAIN_WINDOW_MIN_HEIGHT, MAIN_WINDOW_MIN_WIDTH,
};
use crate::presets;
use crate::scheduler::BackgroundBackupDecision;
use crate::steam::{self, SteamGameCandidate};
use crate::tray;
use crate::{backup, cloud, scheduler};
use chrono::Local;
use eframe::egui;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone, Debug, Default)]
pub(crate) struct GameForm {
    pub name: String,
    pub save_path: String,
    pub max_backups: String,
    pub auto_cleanup_enabled: bool,
    pub backup_storage_mode: BackupStorageMode,
    pub auto_backup_enabled: bool,
    pub auto_backup_interval_value: String,
    pub auto_backup_interval_unit: AutoBackupIntervalUnit,
    pub change_reminder_enabled: bool,
    pub preset_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActiveList {
    Games,
    Backups,
}

#[derive(Clone, Debug)]
pub(crate) enum ConfirmAction {
    BackupEmptySaveDir { game_id: String },
    RestoreBackup { backup_path: PathBuf },
    DeleteBackups { backup_paths: Vec<PathBuf> },
    DeleteGame { game_id: String },
}

#[derive(Clone, Debug)]
pub(crate) enum SteamScanAction {
    AddSelected { app_id: String, save_path: PathBuf },
    AddAll,
    Rescan,
    Close,
}

#[derive(Clone, Debug)]
pub(crate) struct SteamScanDialogState {
    pub(crate) open: bool,
    pub(crate) candidates: Vec<SteamGameCandidate>,
    pub(crate) selected_app_id: Option<String>,
    pub(crate) selected_save_path: Option<PathBuf>,
    pub(crate) pending_action: Option<SteamScanAction>,
    pub(crate) list_width: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct HelpWindowState {
    pub(crate) open: bool,
    pub(crate) selected_topic_id: String,
    pub(crate) search: String,
    pub(crate) search_mode: bool,
}

impl HelpWindowState {
    pub(crate) fn new() -> Self {
        Self {
            open: true,
            selected_topic_id: crate::help::default_topic_id().to_owned(),
            search: String::new(),
            search_mode: false,
        }
    }
}

impl SteamScanDialogState {
    pub(crate) fn from_candidates(candidates: Vec<SteamGameCandidate>) -> Self {
        let selected_app_id = candidates.first().map(|candidate| candidate.app_id.clone());
        let selected_save_path = selected_app_id
            .as_ref()
            .and_then(|app_id| {
                candidates
                    .iter()
                    .find(|candidate| &candidate.app_id == app_id)
            })
            .and_then(SteamGameCandidate::recommended_save_path);

        Self {
            open: true,
            candidates,
            selected_app_id,
            selected_save_path,
            pending_action: None,
            list_width: 380.0,
        }
    }

    pub(crate) fn select_candidate(&mut self, app_id: String) {
        self.selected_app_id = Some(app_id.clone());
        self.selected_save_path = self
            .candidates
            .iter()
            .find(|candidate| candidate.app_id == app_id)
            .and_then(SteamGameCandidate::recommended_save_path);
    }

    pub(crate) fn selected_candidate(&self) -> Option<&SteamGameCandidate> {
        self.selected_app_id.as_ref().and_then(|app_id| {
            self.candidates
                .iter()
                .find(|candidate| &candidate.app_id == app_id)
        })
    }
}

pub struct GameSaveApp {
    pub(crate) config: AppConfig,
    pub(crate) selected_game_id: Option<String>,
    pub(crate) backups: Vec<BackupEntry>,
    pub(crate) selected_backup_path: Option<PathBuf>,
    pub(crate) selected_backup_paths: BTreeSet<PathBuf>,
    pub(crate) backup_delete_selection_mode: bool,
    pub(crate) active_list: ActiveList,
    pub(crate) backup_label: String,
    pub(crate) status: StatusMessage,
    pub(crate) presets: Vec<PresetGame>,
    pub(crate) show_game_dialog: bool,
    pub(crate) show_steam_scan_dialog: bool,
    pub(crate) editing_game_id: Option<String>,
    pub(crate) game_form: GameForm,
    pub(crate) confirm_action: Option<ConfirmAction>,
    pub(crate) delete_backups_with_game: bool,
    pub(crate) steam_candidates: Vec<SteamGameCandidate>,
    pub(crate) selected_steam_app_id: Option<String>,
    pub(crate) selected_steam_save_path: Option<PathBuf>,
    pub(crate) steam_scan_state: Option<Arc<Mutex<SteamScanDialogState>>>,
    pub(crate) app_icon: Option<Arc<egui::IconData>>,
    pub(crate) main_hwnd: Option<tray::WindowHandle>,
    pub(crate) show_close_behavior_dialog: bool,
    pub(crate) show_shortcut_settings_dialog: bool,
    pub(crate) help_window_state: Option<Arc<Mutex<HelpWindowState>>>,
    pub(crate) force_exit_requested: bool,
    pub(crate) last_window_settings_save: Instant,
    pub(crate) last_background_check: Instant,
}

impl GameSaveApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        app_icon: Option<Arc<egui::IconData>>,
        loaded_config: Option<config::ConfigLoadResult>,
    ) -> Self {
        configure_fonts(&cc.egui_ctx);
        let main_hwnd = tray::hwnd_from_creation_context(cc);
        if let Some(hwnd) = main_hwnd {
            tray::init(hwnd);
        }

        let logger_error = logger::init().err();

        let loaded = match loaded_config {
            Some(loaded) => loaded,
            None => match config::load_or_create_config() {
                Ok(loaded) => loaded,
                Err(err) => {
                    logger::error(format!("Configuration load failed: {err}"));
                    let config = config::default_config().unwrap_or_else(|_| AppConfig {
                        backup_root: PathBuf::from("backups"),
                        games: Vec::new(),
                        settings: Default::default(),
                    });
                    let language = config.settings.language;
                    let status = StatusMessage::error(match language {
                        Language::ZhCn => format!(
                            "配置读取失败，已使用临时默认配置: {}",
                            err.user_message_for_language(language)
                        ),
                        Language::EnUs => format!(
                            "Configuration load failed; using a temporary default: {}",
                            err.user_message_for_language(language)
                        ),
                    });
                    return Self::from_config(cc, app_icon, main_hwnd, config, status);
                }
            },
        };
        let config = loaded.config;
        let status = if let Some(recovery) = loaded.recovery {
            logger::warn(format!("Configuration recovery used: {recovery:?}"));
            match (config.settings.language, recovery) {
                (Language::ZhCn, config::ConfigLoadRecovery::Backup) => {
                    StatusMessage::warning("配置文件已从 .bak 备份恢复")
                }
                (Language::EnUs, config::ConfigLoadRecovery::Backup) => {
                    StatusMessage::warning("Configuration restored from the .bak backup")
                }
                (Language::ZhCn, config::ConfigLoadRecovery::Default) => {
                    StatusMessage::warning("配置文件及其备份均不可用，已创建默认配置")
                }
                (Language::EnUs, config::ConfigLoadRecovery::Default) => StatusMessage::warning(
                    "The configuration and its backup were unusable; defaults were created",
                ),
            }
        } else if let Some(err) = logger_error {
            match config.settings.language {
                Language::ZhCn => StatusMessage::warning(format!(
                    "日志初始化失败: {}",
                    err.user_message_for_language(Language::ZhCn)
                )),
                Language::EnUs => StatusMessage::warning(format!(
                    "Log initialization failed: {}",
                    err.user_message_for_language(Language::EnUs)
                )),
            }
        } else {
            StatusMessage::info(t(config.settings.language, T::Ready))
        };

        Self::from_config(cc, app_icon, main_hwnd, config, status)
    }

    fn from_config(
        _cc: &eframe::CreationContext<'_>,
        app_icon: Option<Arc<egui::IconData>>,
        main_hwnd: Option<tray::WindowHandle>,
        config: AppConfig,
        status: StatusMessage,
    ) -> Self {
        tray::set_language(config.settings.language);
        let selected_game_id = config.games.first().map(|game| game.id.clone());
        let mut app = Self {
            config,
            selected_game_id,
            backups: Vec::new(),
            selected_backup_path: None,
            selected_backup_paths: BTreeSet::new(),
            backup_delete_selection_mode: false,
            active_list: ActiveList::Games,
            backup_label: String::new(),
            status,
            presets: presets::built_in_presets(),
            show_game_dialog: false,
            show_steam_scan_dialog: false,
            editing_game_id: None,
            game_form: GameForm::default(),
            confirm_action: None,
            delete_backups_with_game: false,
            steam_candidates: Vec::new(),
            selected_steam_app_id: None,
            selected_steam_save_path: None,
            steam_scan_state: None,
            app_icon,
            main_hwnd,
            show_close_behavior_dialog: false,
            show_shortcut_settings_dialog: false,
            help_window_state: None,
            force_exit_requested: false,
            last_window_settings_save: Instant::now(),
            last_background_check: Instant::now(),
        };
        app.refresh_backups();
        app
    }

    pub(crate) fn language(&self) -> Language {
        self.config.settings.language
    }

    pub(crate) fn text(&self, text: T) -> &'static str {
        t(self.language(), text)
    }

    pub(crate) fn toggle_language(&mut self, ctx: &egui::Context) {
        self.config.settings.language = self.config.settings.language.toggled();
        self.status = StatusMessage::success(self.text(T::LanguageChanged));
        self.save_config();
        tray::set_language(self.language());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            self.text(T::AppTitle).to_owned(),
        ));
    }

    pub(crate) fn selected_game(&self) -> Option<&GameConfig> {
        self.selected_game_id
            .as_ref()
            .and_then(|id| self.config.games.iter().find(|game| &game.id == id))
    }

    pub(crate) fn select_game(&mut self, game_id: String) {
        self.selected_game_id = Some(game_id);
        self.active_list = ActiveList::Games;
        self.selected_backup_path = None;
        self.selected_backup_paths.clear();
        self.backup_delete_selection_mode = false;
        self.backup_label.clear();
        self.refresh_backups();
    }

    pub(crate) fn refresh_backups(&mut self) {
        let Some(game) = self.selected_game().cloned() else {
            self.backups.clear();
            self.selected_backup_path = None;
            self.selected_backup_paths.clear();
            self.backup_delete_selection_mode = false;
            return;
        };

        match backup::scan_backups(&self.config, &game) {
            Ok(backups) => {
                self.backups = backups;
                self.selected_backup_paths
                    .retain(|path| self.backups.iter().any(|entry| &entry.path == path));
                self.sync_primary_backup_selection();
            }
            Err(err) => {
                self.backups.clear();
                self.selected_backup_path = None;
                self.selected_backup_paths.clear();
                self.backup_delete_selection_mode = false;
                self.set_error(err);
            }
        }
    }

    pub(crate) fn single_selected_backup_path(&self) -> Option<PathBuf> {
        self.selected_backup_path.clone()
    }

    pub(crate) fn select_single_backup(&mut self, backup_path: PathBuf) {
        self.active_list = ActiveList::Backups;
        self.selected_backup_path = Some(backup_path);
    }

    pub(crate) fn set_backup_delete_selected(&mut self, backup_path: PathBuf, selected: bool) {
        self.active_list = ActiveList::Backups;
        self.backup_delete_selection_mode = true;
        if selected {
            self.selected_backup_paths.insert(backup_path);
        } else {
            self.selected_backup_paths.remove(&backup_path);
        }
    }

    pub(crate) fn all_backups_selected_for_delete(&self) -> bool {
        !self.backups.is_empty()
            && self.selected_backup_paths.len() == self.backups.len()
            && self
                .backups
                .iter()
                .all(|entry| self.selected_backup_paths.contains(&entry.path))
    }

    pub(crate) fn select_all_backups_for_delete(&mut self) {
        self.active_list = ActiveList::Backups;
        self.backup_delete_selection_mode = true;
        self.selected_backup_paths = self
            .backups
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
    }

    pub(crate) fn clear_backup_delete_selection(&mut self) {
        self.active_list = ActiveList::Backups;
        self.selected_backup_paths.clear();
    }

    pub(crate) fn toggle_backup_delete_selection_mode(&mut self) {
        self.active_list = ActiveList::Backups;
        self.backup_delete_selection_mode = !self.backup_delete_selection_mode;
        if !self.backup_delete_selection_mode {
            self.selected_backup_paths.clear();
        }
    }

    fn sync_primary_backup_selection(&mut self) {
        if let Some(selected_path) = &self.selected_backup_path {
            if !self
                .backups
                .iter()
                .any(|entry| &entry.path == selected_path)
            {
                self.selected_backup_path = None;
            }
        }
    }

    pub(crate) fn save_config(&mut self) {
        match config::save_config(&self.config) {
            Ok(()) => {
                logger::info("配置保存完成");
            }
            Err(err) => {
                logger::error(format!("配置保存失败: {err}"));
                self.set_error(err);
            }
        }
    }

    pub(crate) fn open_add_game_dialog(&mut self) {
        self.editing_game_id = None;
        self.game_form = GameForm {
            max_backups: "20".to_owned(),
            auto_cleanup_enabled: true,
            backup_storage_mode: BackupStorageMode::Incremental,
            auto_backup_enabled: false,
            auto_backup_interval_value: "24".to_owned(),
            auto_backup_interval_unit: AutoBackupIntervalUnit::Hours,
            change_reminder_enabled: true,
            ..Default::default()
        };
        self.show_game_dialog = true;
    }

    pub(crate) fn open_edit_game_dialog(&mut self) {
        let Some(game) = self.selected_game().cloned() else {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "请先选择一个游戏",
                Language::EnUs => "Select a game first",
            });
            return;
        };

        self.editing_game_id = Some(game.id);
        self.game_form = GameForm {
            name: game.name,
            save_path: game.save_path.to_string_lossy().to_string(),
            max_backups: game
                .max_backups
                .map(|value| value.to_string())
                .unwrap_or_default(),
            auto_cleanup_enabled: game.auto_cleanup_enabled,
            backup_storage_mode: game.backup_storage_mode,
            auto_backup_enabled: game.auto_backup.enabled,
            auto_backup_interval_value: game.auto_backup.display_interval_value().to_string(),
            auto_backup_interval_unit: game.auto_backup.display_interval_unit(),
            change_reminder_enabled: game.auto_backup.change_reminder_enabled,
            preset_index: None,
        };
        self.show_game_dialog = true;
    }

    pub(crate) fn submit_game_form(&mut self) {
        match self.build_game_from_form() {
            Ok(mut game) => {
                let saved_game_id;
                if let Some(editing_id) = self.editing_game_id.clone() {
                    game.id = editing_id.clone();
                    saved_game_id = editing_id.clone();
                    if let Some(existing) = self
                        .config
                        .games
                        .iter_mut()
                        .find(|item| item.id == editing_id)
                    {
                        *existing = game;
                    }
                } else {
                    self.selected_game_id = Some(game.id.clone());
                    saved_game_id = game.id.clone();
                    self.selected_backup_path = None;
                    self.selected_backup_paths.clear();
                    self.backup_delete_selection_mode = false;
                    self.config.games.push(game);
                }

                self.schedule_game_auto_backup_from_now(&saved_game_id);
                self.show_game_dialog = false;
                self.save_config();
                self.schedule_background_check_now();
                self.refresh_backups();
                self.status = StatusMessage::success(match self.language() {
                    Language::ZhCn => "游戏配置已保存",
                    Language::EnUs => "Game config saved",
                });
            }
            Err(err) => self.set_error(err),
        }
    }

    pub(crate) fn build_game_from_form(&self) -> AppResult<GameConfig> {
        let name = self.game_form.name.trim();
        if name.is_empty() {
            return Err(AppError::message(match self.language() {
                Language::ZhCn => "游戏名称不能为空",
                Language::EnUs => "Game name cannot be empty",
            }));
        }
        let editing_id = self.editing_game_id.as_deref();
        if self
            .config
            .games
            .iter()
            .any(|game| game.name == name && Some(game.id.as_str()) != editing_id)
        {
            return Err(AppError::message(match self.language() {
                Language::ZhCn => "已存在同名游戏配置，请使用不同名称",
                Language::EnUs => "A game with this name already exists",
            }));
        }

        let save_path = self.game_form.save_path.trim();
        if save_path.is_empty() {
            return Err(AppError::message(match self.language() {
                Language::ZhCn => "存档目录不能为空",
                Language::EnUs => "Save folder cannot be empty",
            }));
        }

        let max_backups = if self.game_form.max_backups.trim().is_empty() {
            None
        } else {
            let value = self
                .game_form
                .max_backups
                .trim()
                .parse::<usize>()
                .map_err(|_| {
                    AppError::message(match self.language() {
                        Language::ZhCn => "最大备份数量必须是正整数或留空",
                        Language::EnUs => "Max backups must be a positive integer or blank",
                    })
                })?;
            if value == 0 {
                return Err(AppError::message(match self.language() {
                    Language::ZhCn => "最大备份数量不能为 0；如不限制请留空",
                    Language::EnUs => "Max backups cannot be 0; leave blank for unlimited",
                }));
            }
            Some(value)
        };

        let auto_backup_interval_value = self
            .game_form
            .auto_backup_interval_value
            .trim()
            .parse::<u64>()
            .map_err(|_| {
                AppError::message(match self.language() {
                    Language::ZhCn => "自动备份间隔必须是正整数",
                    Language::EnUs => "Auto backup interval must be a positive integer",
                })
            })?;
        if auto_backup_interval_value == 0 {
            return Err(AppError::message(match self.language() {
                Language::ZhCn => "自动备份间隔不能为 0",
                Language::EnUs => "Auto backup interval cannot be 0",
            }));
        }
        let auto_backup_interval_minutes = match self.game_form.auto_backup_interval_unit {
            AutoBackupIntervalUnit::Minutes => auto_backup_interval_value,
            AutoBackupIntervalUnit::Hours => {
                auto_backup_interval_value.checked_mul(60).ok_or_else(|| {
                    AppError::message(match self.language() {
                        Language::ZhCn => "自动备份间隔过大",
                        Language::EnUs => "Auto backup interval is too large",
                    })
                })?
            }
        };
        let auto_backup_interval_hours = auto_backup_interval_minutes.div_ceil(60).max(1);

        Ok(GameConfig {
            id: Uuid::new_v4().to_string(),
            name: name.to_owned(),
            save_path: PathBuf::from(save_path),
            max_backups,
            auto_cleanup_enabled: self.game_form.auto_cleanup_enabled,
            backup_storage_mode: self.game_form.backup_storage_mode,
            steam_link: self
                .editing_game_id
                .as_ref()
                .and_then(|editing_id| self.config.games.iter().find(|game| &game.id == editing_id))
                .and_then(|game| game.steam_link.clone()),
            auto_backup: AutoBackupConfig {
                enabled: self.game_form.auto_backup_enabled,
                interval_hours: auto_backup_interval_hours,
                interval_minutes: Some(auto_backup_interval_minutes),
                interval_unit: self.game_form.auto_backup_interval_unit,
                change_reminder_enabled: self.game_form.change_reminder_enabled,
                last_auto_backup_at: self
                    .editing_game_id
                    .as_ref()
                    .and_then(|editing_id| {
                        self.config.games.iter().find(|game| &game.id == editing_id)
                    })
                    .and_then(|game| game.auto_backup.last_auto_backup_at.clone()),
                last_reminded_snapshot_hash: self
                    .editing_game_id
                    .as_ref()
                    .and_then(|editing_id| {
                        self.config.games.iter().find(|game| &game.id == editing_id)
                    })
                    .and_then(|game| game.auto_backup.last_reminded_snapshot_hash.clone()),
                next_auto_backup_at: self
                    .editing_game_id
                    .as_ref()
                    .and_then(|editing_id| {
                        self.config.games.iter().find(|game| &game.id == editing_id)
                    })
                    .and_then(|game| game.auto_backup.next_auto_backup_at.clone()),
            },
        })
    }

    pub(crate) fn start_backup(&mut self, allow_empty: bool) {
        // TODO: Move backup creation to a background task and report copy progress to the UI.
        let Some(game) = self.selected_game().cloned() else {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "请先添加或选择一个游戏",
                Language::EnUs => "Add or select a game first",
            });
            return;
        };

        let label = self.backup_label.trim().to_owned();
        let label = if label.is_empty() {
            None
        } else {
            Some(label.as_str())
        };
        let cloud_warning = self.cloud_conflict_warning(&game);
        match backup::create_backup(
            &self.config,
            &game,
            backup::BackupCreateOptions::manual(label, self.language(), allow_empty),
        ) {
            Ok(entry) => {
                self.backup_label.clear();
                self.selected_backup_path = Some(entry.path.clone());
                self.selected_backup_paths.clear();
                self.backup_delete_selection_mode = false;
                self.sync_auto_backup_after_manual_backup(&game.id);
                self.refresh_backups();
                let message = self.backup_complete_message(&entry);
                self.status = if let Some(warning) = cloud_warning {
                    StatusMessage::warning(format!("{message}; {warning}"))
                } else {
                    StatusMessage::success(message)
                };
            }
            Err(AppError::EmptySaveDir { .. }) if !allow_empty => {
                self.confirm_action = Some(ConfirmAction::BackupEmptySaveDir { game_id: game.id });
            }
            Err(err) => self.set_error(err),
        }
    }

    pub(crate) fn restore_selected_backup(&mut self, backup_path: &PathBuf) {
        // TODO: Move restore materialization and directory replacement to a background task.
        let Some(game) = self.selected_game().cloned() else {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "请先选择游戏",
                Language::EnUs => "Select a game first",
            });
            return;
        };
        let Some(entry) = self
            .backups
            .iter()
            .find(|item| &item.path == backup_path)
            .cloned()
        else {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "请先选择有效备份",
                Language::EnUs => "Select a valid backup first",
            });
            return;
        };

        let cloud_warning = self.cloud_conflict_warning(&game);
        match backup::restore_backup(&self.config, &game, &entry) {
            Ok(pre_restore) => {
                self.refresh_backups();
                let message = match self.language() {
                    Language::ZhCn => format!(
                        "恢复完成，恢复前自动备份已保存到 {}",
                        pre_restore.path.display()
                    ),
                    Language::EnUs => format!(
                        "Restore complete; safety backup saved to {}",
                        pre_restore.path.display()
                    ),
                };
                self.status = if let Some(warning) = cloud_warning {
                    StatusMessage::warning(format!("{message}; {warning}"))
                } else {
                    StatusMessage::success(message)
                };
            }
            Err(err) => self.set_error(err),
        }
    }

    pub(crate) fn delete_selected_backups(&mut self, backup_paths: &[PathBuf]) {
        let requested: BTreeSet<PathBuf> = backup_paths.iter().cloned().collect();
        let entries: Vec<BackupEntry> = self
            .backups
            .iter()
            .filter(|item| requested.contains(&item.path))
            .cloned()
            .collect();

        if entries.is_empty() {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "请先选择有效备份",
                Language::EnUs => "Select a valid backup first",
            });
            return;
        }

        let mut deleted = 0usize;
        for entry in entries {
            if let Err(err) = backup::delete_backup(&entry) {
                self.set_error(err);
                self.refresh_backups();
                return;
            }
            deleted += 1;
        }

        self.selected_backup_path = None;
        self.selected_backup_paths.clear();
        self.backup_delete_selection_mode = false;
        self.refresh_backups();
        self.status = StatusMessage::success(match self.language() {
            Language::ZhCn => {
                if deleted == 1 {
                    "备份已删除".to_owned()
                } else {
                    format!("已删除 {deleted} 个备份")
                }
            }
            Language::EnUs => {
                if deleted == 1 {
                    "Backup deleted".to_owned()
                } else {
                    format!("Deleted {deleted} backups")
                }
            }
        });
    }

    pub(crate) fn delete_game(&mut self, game_id: &str) {
        let Some(index) = self.config.games.iter().position(|game| game.id == game_id) else {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "游戏配置不存在",
                Language::EnUs => "Game config does not exist",
            });
            return;
        };
        let game = self.config.games[index].clone();

        if self.delete_backups_with_game {
            if let Err(err) = backup::delete_game_backup_dir(&self.config, &game) {
                self.set_error(err);
                return;
            }
        }

        self.config.games.remove(index);
        self.selected_game_id = self.config.games.first().map(|game| game.id.clone());
        self.selected_backup_path = None;
        self.selected_backup_paths.clear();
        self.backup_delete_selection_mode = false;
        self.delete_backups_with_game = false;
        self.save_config();
        self.refresh_backups();
        self.status = StatusMessage::success(match self.language() {
            Language::ZhCn => "游戏配置已删除",
            Language::EnUs => "Game config deleted",
        });
    }

    pub(crate) fn set_backup_root(&mut self, path: PathBuf) {
        let old_backup_root = self.config.backup_root.clone();
        self.config.backup_root = path;
        match config::save_config(&self.config) {
            Ok(()) => {
                logger::info("备份根目录保存完成");
                self.refresh_backups();
                self.status = StatusMessage::success(match self.language() {
                    Language::ZhCn => "备份根目录已更新",
                    Language::EnUs => "Backup root updated",
                });
            }
            Err(err) => {
                self.config.backup_root = old_backup_root;
                logger::error(format!("备份根目录保存失败: {err}"));
                self.set_error(err);
            }
        }
    }

    pub(crate) fn change_app_data_dir(&mut self, selected_dir: PathBuf) {
        // TODO: Run data-directory migration off the egui thread with progress reporting.
        match config::migrate_app_data_dir(&selected_dir, &mut self.config) {
            Ok(path) => {
                self.refresh_backups();
                self.status = match self.language() {
                    Language::ZhCn => StatusMessage::success(format!(
                        "数据目录已迁移到 {}，旧目录已删除",
                        path.display()
                    )),
                    Language::EnUs => StatusMessage::success(format!(
                        "Data folder moved to {}; old folder removed",
                        path.display()
                    )),
                };
            }
            Err(err) => self.set_error(err),
        }
    }

    pub(crate) fn open_steam_scan_dialog(&mut self) {
        // TODO: Run Steam library discovery in a background task.
        match steam::scan_installed_games() {
            Ok(candidates) => {
                let state = SteamScanDialogState::from_candidates(candidates.clone());
                self.steam_candidates = candidates;
                self.selected_steam_app_id = state.selected_app_id.clone();
                self.selected_steam_save_path = state.selected_save_path.clone();
                self.steam_scan_state = Some(Arc::new(Mutex::new(state)));

                if self.selected_steam_app_id.is_some() {
                    self.status = match self.language() {
                        Language::ZhCn => StatusMessage::success(format!(
                            "已扫描到 {} 个 Steam 游戏",
                            self.steam_candidates.len()
                        )),
                        Language::EnUs => StatusMessage::success(format!(
                            "Found {} Steam games",
                            self.steam_candidates.len()
                        )),
                    };
                } else {
                    self.selected_steam_app_id = None;
                    self.selected_steam_save_path = None;
                    self.status = StatusMessage::warning(match self.language() {
                        Language::ZhCn => "未扫描到已安装的 Steam 游戏",
                        Language::EnUs => "No installed Steam games found",
                    });
                }
                self.show_steam_scan_dialog = true;
            }
            Err(err) => self.set_error(err),
        }
    }

    pub(crate) fn selected_steam_candidate(&self) -> Option<&SteamGameCandidate> {
        self.selected_steam_app_id.as_ref().and_then(|app_id| {
            self.steam_candidates
                .iter()
                .find(|candidate| &candidate.app_id == app_id)
        })
    }

    pub(crate) fn process_steam_scan_window_actions(&mut self) {
        let Some(state_ref) = self.steam_scan_state.clone() else {
            return;
        };

        let (snapshot, action) = match state_ref.lock() {
            Ok(mut state) => {
                let snapshot = state.clone();
                let action = state.pending_action.take();
                (snapshot, action)
            }
            Err(_) => {
                self.close_steam_scan_dialog();
                self.status = StatusMessage::error(match self.language() {
                    Language::ZhCn => "Steam 扫描窗口状态异常，已关闭窗口",
                    Language::EnUs => "Steam scan window state failed; window closed",
                });
                return;
            }
        };

        self.steam_candidates = snapshot.candidates.clone();
        self.selected_steam_app_id = snapshot.selected_app_id.clone();
        self.selected_steam_save_path = snapshot.selected_save_path.clone();

        match action {
            Some(SteamScanAction::AddSelected { app_id, save_path }) => {
                self.selected_steam_app_id = Some(app_id);
                self.selected_steam_save_path = Some(save_path);
                self.add_selected_steam_game();
                if !self.show_steam_scan_dialog {
                    self.steam_scan_state = None;
                }
            }
            Some(SteamScanAction::AddAll) => {
                self.add_all_steam_games_with_save_paths();
            }
            Some(SteamScanAction::Rescan) => {
                self.open_steam_scan_dialog();
            }
            Some(SteamScanAction::Close) => {
                self.close_steam_scan_dialog();
            }
            None if !snapshot.open => {
                self.close_steam_scan_dialog();
            }
            None => {}
        }
    }

    fn close_steam_scan_dialog(&mut self) {
        self.show_steam_scan_dialog = false;
        self.steam_scan_state = None;
    }

    pub(crate) fn add_selected_steam_game(&mut self) {
        let Some(candidate) = self.selected_steam_candidate().cloned() else {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "请先选择一个 Steam 游戏",
                Language::EnUs => "Select a Steam game first",
            });
            return;
        };
        let Some(save_path) = self.selected_steam_save_path.clone() else {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "请先选择或手动指定存档目录",
                Language::EnUs => "Choose or manually set a save folder first",
            });
            return;
        };
        if self
            .config
            .games
            .iter()
            .any(|game| game.name == candidate.name)
        {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "游戏列表中已存在同名游戏",
                Language::EnUs => "A game with this name already exists",
            });
            return;
        }

        let game = GameConfig {
            id: Uuid::new_v4().to_string(),
            name: candidate.name.clone(),
            save_path,
            max_backups: Some(20),
            auto_cleanup_enabled: true,
            backup_storage_mode: BackupStorageMode::Incremental,
            steam_link: Some(steam_link_from_candidate(&candidate)),
            auto_backup: Default::default(),
        };

        self.selected_game_id = Some(game.id.clone());
        self.selected_backup_path = None;
        self.selected_backup_paths.clear();
        self.backup_delete_selection_mode = false;
        self.backup_label.clear();
        self.config.games.push(game);
        self.show_steam_scan_dialog = false;
        self.save_config();
        self.refresh_backups();
        self.status = match self.language() {
            Language::ZhCn => {
                StatusMessage::success(format!("已加入 Steam 游戏：{}", candidate.name))
            }
            Language::EnUs => {
                StatusMessage::success(format!("Added Steam game: {}", candidate.name))
            }
        };
    }

    pub(crate) fn add_all_steam_games_with_save_paths(&mut self) {
        let mut added = 0usize;
        let mut skipped_duplicates = 0usize;
        let mut skipped_without_path = 0usize;

        for candidate in self.steam_candidates.clone() {
            if self
                .config
                .games
                .iter()
                .any(|game| game.name == candidate.name)
            {
                skipped_duplicates += 1;
                continue;
            }

            let Some(save_path) = candidate.recommended_save_path() else {
                skipped_without_path += 1;
                continue;
            };

            let game = GameConfig {
                id: Uuid::new_v4().to_string(),
                name: candidate.name.clone(),
                save_path,
                max_backups: Some(20),
                auto_cleanup_enabled: true,
                backup_storage_mode: BackupStorageMode::Incremental,
                steam_link: Some(steam_link_from_candidate(&candidate)),
                auto_backup: Default::default(),
            };

            self.selected_game_id = Some(game.id.clone());
            self.config.games.push(game);
            added += 1;
        }

        if added > 0 {
            self.selected_backup_path = None;
            self.selected_backup_paths.clear();
            self.backup_delete_selection_mode = false;
            self.backup_label.clear();
            self.save_config();
            self.refresh_backups();
            self.status = match self.language() {
                Language::ZhCn => StatusMessage::success(format!(
                    "已加入 {added} 个 Steam 游戏；跳过 {skipped_duplicates} 个已存在，{skipped_without_path} 个无候选目录"
                )),
                Language::EnUs => StatusMessage::success(format!(
                    "Added {added} Steam games; skipped {skipped_duplicates} existing, {skipped_without_path} without candidates"
                )),
            };
        } else {
            self.status = match self.language() {
                Language::ZhCn => StatusMessage::warning(format!(
                    "没有可批量加入的游戏；跳过 {skipped_duplicates} 个已存在，{skipped_without_path} 个无候选目录"
                )),
                Language::EnUs => StatusMessage::warning(format!(
                    "No games to batch add; skipped {skipped_duplicates} existing, {skipped_without_path} without candidates"
                )),
            };
        }
    }

    pub(crate) fn set_error(&mut self, err: AppError) {
        let message = err.user_message_for_language(self.language());
        logger::error(err.to_string());
        self.status = StatusMessage::error(message);
    }

    pub(crate) fn open_path(&mut self, path: PathBuf) {
        match opener::open(&path) {
            Ok(()) => {}
            Err(err) => {
                logger::error(format!("Failed to open folder: {} ({err})", path.display()));
                self.status =
                    StatusMessage::error(open_folder_error_message(self.language(), &path, &err));
            }
        }
    }

    pub(crate) fn open_user_guide(&mut self) {
        if let Some(state_ref) = &self.help_window_state {
            if let Ok(mut state) = state_ref.lock() {
                state.open = true;
            }
        } else {
            self.help_window_state = Some(Arc::new(Mutex::new(HelpWindowState::new())));
        }
        self.status = StatusMessage::success(self.text(T::HelpOpened));
    }

    pub(crate) fn open_shortcut_settings(&mut self) {
        self.show_shortcut_settings_dialog = true;
        self.status = StatusMessage::success(self.text(T::ShortcutSettingsOpened));
    }

    pub(crate) fn backup_complete_message(&self, entry: &BackupEntry) -> String {
        let stored = entry
            .stored_size
            .map(format_size)
            .unwrap_or_else(|| format_size(entry.total_size));
        match self.language() {
            Language::ZhCn => format!(
                "备份完成：{} 个文件，原始大小 {}，存储大小 {}",
                entry.file_count,
                format_size(entry.total_size),
                stored
            ),
            Language::EnUs => format!(
                "Backup complete: {} files, source {}, stored {}",
                entry.file_count,
                format_size(entry.total_size),
                stored
            ),
        }
    }

    pub(crate) fn cloud_conflict_warning(&self, game: &GameConfig) -> Option<String> {
        match cloud::check_steam_cloud_conflict(game) {
            Ok(Some(conflict)) => Some(match self.language() {
                Language::ZhCn => format!(
                    "Steam Cloud 本地缓存与存档目录的最新修改时间不一致，请确认是否存在云同步冲突: {}",
                    conflict.cloud_path.display()
                ),
                Language::EnUs => format!(
                    "Steam Cloud local cache has a different latest modified time; check for sync conflicts: {}",
                    conflict.cloud_path.display()
                ),
            }),
            Ok(None) => None,
            Err(err) => Some(match self.language() {
                Language::ZhCn => format!(
                    "Steam Cloud 冲突检测失败: {}",
                    err.user_message_for_language(Language::ZhCn)
                ),
                Language::EnUs => format!(
                    "Steam Cloud conflict check failed: {}",
                    err.user_message_for_language(Language::EnUs)
                ),
            }),
        }
    }

    pub(crate) fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if !self.config.settings.keyboard_shortcuts_enabled {
            return;
        }

        if ctx.wants_keyboard_input() {
            return;
        }

        if ctx.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::S,
            ))
        }) {
            self.start_backup(false);
        }

        if ctx.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::R,
            ))
        }) {
            if let Some(path) = self.single_selected_backup_path() {
                self.confirm_action = Some(ConfirmAction::RestoreBackup { backup_path: path });
            }
        }

        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            self.navigate_active_list(1);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            self.navigate_active_list(-1);
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)) {
            self.active_list = ActiveList::Games;
        }
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight))
            && !self.backups.is_empty()
        {
            self.active_list = ActiveList::Backups;
        }
    }

    fn navigate_active_list(&mut self, delta: isize) {
        match self.active_list {
            ActiveList::Games => {
                if self.config.games.is_empty() {
                    return;
                }
                let current = self
                    .selected_game_id
                    .as_ref()
                    .and_then(|id| self.config.games.iter().position(|game| &game.id == id))
                    .unwrap_or(0);
                let next = offset_index(current, self.config.games.len(), delta);
                let game_id = self.config.games[next].id.clone();
                self.select_game(game_id);
            }
            ActiveList::Backups => {
                if self.backups.is_empty() {
                    return;
                }
                let current = self
                    .selected_backup_path
                    .as_ref()
                    .and_then(|path| self.backups.iter().position(|entry| &entry.path == path))
                    .unwrap_or(0);
                let next = offset_index(current, self.backups.len(), delta);
                self.select_single_backup(self.backups[next].path.clone());
            }
        }
    }

    pub(crate) fn run_background_checks(&mut self) {
        // TODO: Throttle or move save-directory hashing off the egui thread.
        if self.last_background_check.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_background_check = Instant::now();

        let mut config_changed = false;
        let game_ids: Vec<String> = self
            .config
            .games
            .iter()
            .map(|game| game.id.clone())
            .collect();
        for game_id in game_ids {
            let Some(game) = self
                .config
                .games
                .iter()
                .find(|game| game.id == game_id)
                .cloned()
            else {
                continue;
            };

            let change_state = match scheduler::backup_change_state(&self.config, &game) {
                Ok(state) => state,
                Err(err) => {
                    logger::warn(format!(
                        "Background save change check failed for {}: {}",
                        game.name, err
                    ));
                    if self.selected_game_id.as_deref() == Some(game_id.as_str()) {
                        self.status = StatusMessage::warning(match self.language() {
                            Language::ZhCn => format!(
                                "自动备份检查失败：{}",
                                err.user_message_for_language(Language::ZhCn)
                            ),
                            Language::EnUs => format!(
                                "Automatic backup check failed: {}",
                                err.user_message_for_language(Language::EnUs)
                            ),
                        });
                    }
                    continue;
                }
            };

            match scheduler::background_backup_decision(&game, &change_state, Local::now()) {
                BackgroundBackupDecision::AutoBackup { snapshot_hash } => {
                    let backup_result = backup::create_backup(
                        &self.config,
                        &game,
                        backup::BackupCreateOptions::automatic(self.language()),
                    );

                    match backup_result {
                        Ok(entry) => {
                            if let Some(game) =
                                self.config.games.iter_mut().find(|game| game.id == game_id)
                            {
                                let now = Local::now();
                                game.auto_backup.last_auto_backup_at = Some(now.to_rfc3339());
                                if let Some(snapshot_hash) = &snapshot_hash {
                                    game.auto_backup.last_reminded_snapshot_hash =
                                        Some(snapshot_hash.clone());
                                }
                                game.auto_backup.next_auto_backup_at =
                                    scheduler::next_auto_backup_time_string(game, now);
                            }
                            config_changed = true;
                            self.refresh_backups();
                            self.status =
                                StatusMessage::success(self.backup_complete_message(&entry));
                        }
                        Err(err) => {
                            logger::warn(format!(
                                "Automatic backup failed for {}: {}",
                                game.name, err
                            ));
                            if let Some(game) =
                                self.config.games.iter_mut().find(|game| game.id == game_id)
                            {
                                game.auto_backup.next_auto_backup_at =
                                    scheduler::next_auto_backup_time_string(game, Local::now());
                            }
                            config_changed = true;
                            if self.selected_game_id.as_deref() == Some(game_id.as_str()) {
                                let language = self.language();
                                let message = err.user_message_for_language(language);
                                self.status = StatusMessage::warning(match self.language() {
                                    Language::ZhCn => format!("自动备份失败：{message}"),
                                    Language::EnUs => format!("Automatic backup failed: {message}"),
                                });
                            }
                        }
                    }
                    continue;
                }
                BackgroundBackupDecision::Remind { snapshot_hash } => {
                    if let Some(game) = self.config.games.iter_mut().find(|game| game.id == game_id)
                    {
                        game.auto_backup.last_reminded_snapshot_hash = Some(snapshot_hash);
                    }
                    config_changed = true;
                    if self.selected_game_id.as_deref() == Some(game_id.as_str()) {
                        self.status = StatusMessage::warning(match self.language() {
                            Language::ZhCn => "检测到存档有未备份变更".to_owned(),
                            Language::EnUs => {
                                "Detected save changes that have not been backed up".to_owned()
                            }
                        });
                    }
                }
                BackgroundBackupDecision::AdvanceTimer => {
                    if let Some(game) = self.config.games.iter_mut().find(|game| game.id == game_id)
                    {
                        game.auto_backup.next_auto_backup_at =
                            scheduler::next_auto_backup_time_string(game, Local::now());
                    }
                    config_changed = true;
                    match change_state {
                        scheduler::BackupChangeState::NoSaveFolder => {
                            logger::info(format!(
                                "Automatic backup skipped for {}: save folder does not exist",
                                game.name
                            ));
                            if self.selected_game_id.as_deref() == Some(game_id.as_str()) {
                                self.status = StatusMessage::warning(match self.language() {
                                    Language::ZhCn => "自动备份跳过：存档目录不存在".to_owned(),
                                    Language::EnUs => {
                                        "Automatic backup skipped: save folder does not exist"
                                            .to_owned()
                                    }
                                });
                            }
                        }
                        scheduler::BackupChangeState::NoChanges => {
                            logger::info(format!(
                                "Automatic backup skipped for {}: no save changes",
                                game.name
                            ));
                            if self.selected_game_id.as_deref() == Some(game_id.as_str()) {
                                self.status = StatusMessage::info(match self.language() {
                                    Language::ZhCn => "自动备份已跳过：存档没有变化".to_owned(),
                                    Language::EnUs => {
                                        "Automatic backup skipped: no save changes".to_owned()
                                    }
                                });
                            }
                        }
                        scheduler::BackupChangeState::Changed { .. } => {}
                    }
                }
                BackgroundBackupDecision::Skip => {}
            }
        }

        if config_changed {
            self.save_config();
        }
    }

    pub(crate) fn schedule_background_check_now(&mut self) {
        self.last_background_check = Instant::now()
            .checked_sub(Duration::from_secs(5))
            .unwrap_or_else(Instant::now);
    }

    pub(crate) fn sync_auto_backup_after_manual_backup(&mut self, game_id: &str) {
        if let Some(game) = self.config.games.iter_mut().find(|game| game.id == game_id) {
            let now = Local::now();
            game.auto_backup.last_auto_backup_at = Some(now.to_rfc3339());
            game.auto_backup.last_reminded_snapshot_hash = None;
            game.auto_backup.next_auto_backup_at =
                scheduler::next_auto_backup_time_string(game, now);
            self.save_config();
        }
    }

    pub(crate) fn schedule_game_auto_backup_from_now(&mut self, game_id: &str) {
        if let Some(game) = self.config.games.iter_mut().find(|game| game.id == game_id) {
            game.auto_backup.next_auto_backup_at =
                scheduler::next_auto_backup_time_string(game, Local::now());
        }
    }

    pub(crate) fn handle_root_viewport(&mut self, ctx: &egui::Context) {
        self.remember_window_settings(ctx);

        if tray::take_exit_requested() {
            self.force_exit_requested = true;
            self.show_close_behavior_dialog = false;
            self.show_steam_scan_dialog = false;
            self.steam_scan_state = None;
            self.show_shortcut_settings_dialog = false;
            self.help_window_state = None;
            self.save_config();
            tray::shutdown();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if ctx.input(|input| input.viewport().close_requested()) {
            self.remember_window_settings(ctx);
            if self.force_exit_requested {
                self.save_config();
                tray::shutdown();
                return;
            }

            if !self.config.settings.close_behavior_prompted {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.show_close_behavior_dialog = true;
                return;
            }

            match self.config.settings.close_behavior {
                CloseBehavior::MinimizeToTray => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.hide_to_tray();
                }
                CloseBehavior::Exit => {
                    self.save_config();
                    tray::shutdown();
                }
            }
        }
    }

    fn remember_window_settings(&mut self, ctx: &egui::Context) {
        let viewport = ctx.input(|input| input.viewport().clone());
        if viewport.minimized == Some(true) {
            return;
        }

        let mut changed = false;
        if let Some(maximized) = viewport.maximized {
            if self.config.settings.main_window.maximized != maximized {
                self.config.settings.main_window.maximized = maximized;
                changed = true;
            }
        }

        if let Some(rect) = viewport.inner_rect {
            let size = rect.size();
            if size.x >= MAIN_WINDOW_MIN_WIDTH && size.y >= MAIN_WINDOW_MIN_HEIGHT {
                let old_width = self.config.settings.main_window.width.unwrap_or_default();
                let old_height = self.config.settings.main_window.height.unwrap_or_default();
                if (old_width - size.x).abs() > 1.0 || (old_height - size.y).abs() > 1.0 {
                    self.config.settings.main_window.width = Some(size.x);
                    self.config.settings.main_window.height = Some(size.y);
                    changed = true;
                }
            }
        }

        if changed && self.last_window_settings_save.elapsed() >= Duration::from_millis(800) {
            self.last_window_settings_save = Instant::now();
            self.save_config();
        }
    }

    pub(crate) fn hide_to_tray(&mut self) {
        self.save_config();
        if let Some(hwnd) = self.main_hwnd {
            tray::hide_window(hwnd);
            self.status = StatusMessage::info(match self.language() {
                Language::ZhCn => "已最小化到系统托盘，双击托盘图标可恢复窗口",
                Language::EnUs => "Minimized to tray; double-click the tray icon to restore",
            });
        } else {
            self.status = StatusMessage::warning(match self.language() {
                Language::ZhCn => "当前环境无法创建系统托盘，已保持窗口显示",
                Language::EnUs => "System tray is unavailable; keeping the window visible",
            });
        }
    }

    pub(crate) fn choose_close_behavior(&mut self, behavior: CloseBehavior, ctx: &egui::Context) {
        self.config.settings.close_behavior = behavior;
        self.config.settings.close_behavior_prompted = true;
        self.save_config();

        match behavior {
            CloseBehavior::MinimizeToTray => {
                self.show_close_behavior_dialog = false;
                self.hide_to_tray();
            }
            CloseBehavior::Exit => {
                self.show_close_behavior_dialog = false;
                tray::shutdown();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for GameSaveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_embed_viewports(false);
        if self
            .config
            .games
            .iter()
            .any(|game| game.auto_backup.enabled || game.auto_backup.change_reminder_enabled)
        {
            ctx.request_repaint_after(Duration::from_secs(5));
        }
        self.handle_root_viewport(ctx);
        self.handle_keyboard_shortcuts(ctx);
        self.run_background_checks();
        self.process_steam_scan_window_actions();
        self.draw_status_bar(ctx);
        self.draw_game_list(ctx);
        self.draw_main_panel(ctx);
        self.draw_game_dialog(ctx);
        self.draw_steam_scan_dialog(ctx);
        self.draw_confirmation_dialog(ctx);
        self.draw_close_behavior_dialog(ctx);
        self.draw_shortcut_settings_dialog(ctx);
        self.draw_help_window(ctx);
        self.process_steam_scan_window_actions();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_config();
        tray::shutdown();
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let font_candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];

    for path in font_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "windows_chinese".to_owned(),
                egui::FontData::from_owned(bytes),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "windows_chinese".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "windows_chinese".to_owned());
            ctx.set_fonts(fonts);
            break;
        }
    }

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

pub(crate) fn status_color(kind: StatusKind, visuals: &egui::Visuals) -> egui::Color32 {
    match kind {
        StatusKind::Info => visuals.text_color(),
        StatusKind::Success => egui::Color32::from_rgb(36, 128, 72),
        StatusKind::Warning => egui::Color32::from_rgb(166, 116, 20),
        StatusKind::Error => egui::Color32::from_rgb(180, 48, 48),
    }
}

fn offset_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as isize;
    (current as isize + delta).rem_euclid(len) as usize
}

fn open_folder_error_message(
    language: Language,
    path: &Path,
    _error: &dyn std::error::Error,
) -> String {
    match language {
        Language::ZhCn => {
            format!(
                "打开目录失败：{}（系统无法打开或访问该路径）",
                path.display()
            )
        }
        Language::EnUs => format!(
            "Failed to open folder: {} (The system could not open or access this path)",
            path.display()
        ),
    }
}

fn steam_link_from_candidate(candidate: &SteamGameCandidate) -> SteamLink {
    SteamLink {
        app_id: candidate.app_id.clone(),
        cloud_paths: candidate
            .save_paths
            .iter()
            .filter(|path| path.exists && path.is_steam_cloud_cache())
            .map(|path| path.path.clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn open_folder_failure_message_is_localized_and_preserves_path() {
        let path = Path::new(r"D:\Temp\123123");
        let error = std::io::Error::other("IO error");

        let zh = open_folder_error_message(Language::ZhCn, path, &error);
        assert!(zh.contains("打开目录失败"));
        assert!(!zh.contains("IO error"));
        assert!(!zh.contains("I/O error"));
        assert!(zh.contains(r"D:\Temp\123123"));

        let en = open_folder_error_message(Language::EnUs, path, &error);
        assert!(en.contains("Failed to open folder"));
        assert!(!en.contains("打开目录失败"));
        assert!(!en.contains("输入/输出错误"));
        assert!(en.contains(r"D:\Temp\123123"));
    }

    #[test]
    fn background_checks_create_due_auto_backup_through_real_backup_path() {
        let root = tempdir().unwrap();
        let save = root.path().join("save");
        fs::create_dir_all(&save).unwrap();
        fs::write(save.join("slot.sav"), "one").unwrap();

        let mut game = GameConfig {
            id: "game".to_owned(),
            name: "Game".to_owned(),
            save_path: save,
            max_backups: None,
            auto_cleanup_enabled: true,
            backup_storage_mode: BackupStorageMode::Incremental,
            steam_link: None,
            auto_backup: AutoBackupConfig {
                enabled: true,
                interval_hours: 1,
                interval_minutes: Some(1),
                interval_unit: AutoBackupIntervalUnit::Minutes,
                change_reminder_enabled: false,
                last_auto_backup_at: None,
                last_reminded_snapshot_hash: None,
                next_auto_backup_at: Some(
                    (Local::now() - chrono::Duration::minutes(1)).to_rfc3339(),
                ),
            },
        };
        game.auto_backup.next_auto_backup_at = scheduler::next_auto_backup_time_string(
            &game,
            Local::now() - chrono::Duration::minutes(2),
        );

        let config = AppConfig {
            backup_root: root.path().join("backups"),
            games: vec![game],
            settings: Default::default(),
        };
        let mut app = test_app(config);
        app.schedule_background_check_now();

        app.run_background_checks();

        assert_eq!(app.backups.len(), 1);
        assert_eq!(app.backups[0].label.as_deref(), Some("自动备份"));
        assert_eq!(
            app.backups[0].label_kind,
            crate::models::BackupLabelKind::Automatic
        );
        assert_eq!(app.status.kind, StatusKind::Success);
        let saved_game = app
            .config
            .games
            .iter()
            .find(|game| game.id == "game")
            .unwrap();
        assert!(saved_game.auto_backup.last_auto_backup_at.is_some());
        assert!(saved_game.auto_backup.next_auto_backup_at.is_some());
    }

    #[test]
    fn background_checks_skip_due_auto_backup_when_save_is_unchanged() {
        let root = tempdir().unwrap();
        let save = root.path().join("save");
        fs::create_dir_all(&save).unwrap();
        fs::write(save.join("slot.sav"), "one").unwrap();
        let game = GameConfig {
            id: "game".to_owned(),
            name: "Game".to_owned(),
            save_path: save,
            max_backups: None,
            auto_cleanup_enabled: true,
            backup_storage_mode: BackupStorageMode::Incremental,
            steam_link: None,
            auto_backup: AutoBackupConfig {
                enabled: true,
                interval_hours: 1,
                interval_minutes: Some(1),
                interval_unit: AutoBackupIntervalUnit::Minutes,
                change_reminder_enabled: true,
                last_auto_backup_at: None,
                last_reminded_snapshot_hash: None,
                next_auto_backup_at: Some(
                    (Local::now() - chrono::Duration::minutes(1)).to_rfc3339(),
                ),
            },
        };
        let config = AppConfig {
            backup_root: root.path().join("backups"),
            games: vec![game],
            settings: Default::default(),
        };
        let mut app = test_app(config);
        app.schedule_background_check_now();
        app.run_background_checks();
        assert_eq!(app.backups.len(), 1);

        let first_state = app.config.games[0].auto_backup.clone();
        app.config.games[0].auto_backup.next_auto_backup_at =
            Some((Local::now() - chrono::Duration::minutes(1)).to_rfc3339());
        app.schedule_background_check_now();
        app.run_background_checks();

        assert_eq!(app.backups.len(), 1);
        assert_eq!(
            app.config.games[0].auto_backup.last_auto_backup_at,
            first_state.last_auto_backup_at
        );
        assert_eq!(
            app.config.games[0].auto_backup.last_reminded_snapshot_hash,
            first_state.last_reminded_snapshot_hash
        );
        assert_ne!(
            app.config.games[0].auto_backup.next_auto_backup_at,
            first_state.next_auto_backup_at
        );
        assert!(app.status.text.contains("没有变化"));
    }

    fn test_app(config: AppConfig) -> GameSaveApp {
        GameSaveApp {
            config,
            selected_game_id: Some("game".to_owned()),
            backups: Vec::new(),
            selected_backup_path: None,
            selected_backup_paths: BTreeSet::new(),
            backup_delete_selection_mode: false,
            active_list: ActiveList::Games,
            backup_label: String::new(),
            status: StatusMessage::info("test"),
            presets: Vec::new(),
            show_game_dialog: false,
            show_steam_scan_dialog: false,
            editing_game_id: None,
            game_form: GameForm::default(),
            confirm_action: None,
            delete_backups_with_game: false,
            steam_candidates: Vec::new(),
            selected_steam_app_id: None,
            selected_steam_save_path: None,
            steam_scan_state: None,
            app_icon: None,
            main_hwnd: None,
            show_close_behavior_dialog: false,
            show_shortcut_settings_dialog: false,
            help_window_state: None,
            force_exit_requested: false,
            last_window_settings_save: Instant::now(),
            last_background_check: Instant::now(),
        }
    }
}
