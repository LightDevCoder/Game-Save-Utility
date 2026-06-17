use crate::app::{ConfirmAction, GameSaveApp};
use crate::backup;
use crate::config;
use crate::fs_utils::{expand_path, format_size};
use crate::i18n::Text as T;
use crate::models::{
    AutoBackupConfig, AutoBackupIntervalUnit, BackupEntry, BackupStorageKind, BackupStorageMode,
    CloseBehavior, IncrementalBackupKind, Language, StatusMessage,
};
use chrono::{DateTime, Local};
use eframe::egui;

impl GameSaveApp {
    pub(crate) fn draw_main_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 36.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let mut behavior = self.config.settings.close_behavior;
                    ui.add_sized([58.0, 28.0], egui::Label::new(self.text(T::CloseButton)));
                    egui::ComboBox::from_id_source("close_behavior_setting")
                        .width(150.0)
                        .selected_text(match behavior {
                            CloseBehavior::MinimizeToTray => self.text(T::MinimizeToTray),
                            CloseBehavior::Exit => self.text(T::ExitApp),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut behavior,
                                CloseBehavior::MinimizeToTray,
                                self.text(T::MinimizeToTray),
                            );
                            ui.selectable_value(
                                &mut behavior,
                                CloseBehavior::Exit,
                                self.text(T::ExitApp),
                            );
                        });
                    if behavior != self.config.settings.close_behavior {
                        self.config.settings.close_behavior = behavior;
                        self.config.settings.close_behavior_prompted = true;
                        self.save_config();
                        self.status = StatusMessage::success(self.text(T::CloseBehaviorUpdated));
                    }
                    ui.separator();
                    ui.add_sized([56.0, 28.0], egui::Label::new(self.text(T::DataDir)));
                    let reserved_button_width = 390.0;
                    let path_width =
                        (ui.available_width() - reserved_button_width).clamp(150.0, 360.0);
                    match config::app_data_dir() {
                        Ok(path) => {
                            let _ = ui.add_sized(
                                [path_width, 24.0],
                                egui::Label::new(
                                    egui::RichText::new(path.display().to_string()).monospace(),
                                )
                                .truncate(true),
                            );
                        }
                        Err(err) => {
                            ui.add_sized(
                                [path_width, 24.0],
                                egui::Label::new(
                                    egui::RichText::new(err.user_message())
                                        .color(egui::Color32::RED),
                                )
                                .truncate(true),
                            );
                        }
                    }
                    if ui
                        .add_sized([86.0, 28.0], egui::Button::new(self.text(T::MoveData)))
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.change_app_data_dir(path);
                        }
                    }
                    ui.separator();
                    if ui
                        .add_sized(
                            [64.0, 28.0],
                            egui::Button::new(self.text(T::ShortcutSettings)),
                        )
                        .clicked()
                    {
                        self.open_shortcut_settings();
                    }
                    if ui
                        .add_sized([74.0, 28.0], egui::Button::new(self.text(T::Docs)))
                        .clicked()
                    {
                        self.open_user_guide();
                    }
                    if ui
                        .add_sized(
                            [72.0, 28.0],
                            egui::Button::new(self.text(T::SwitchLanguage)),
                        )
                        .clicked()
                    {
                        self.toggle_language(ctx);
                    }
                },
            );
            ui.separator();

            if let Some(game) = self.selected_game().cloned() {
                ui.heading(&game.name);
                ui.add_space(4.0);

                egui::Grid::new("selected_game_info")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(self.text(T::SavePath));
                        ui.monospace(expand_path(&game.save_path).display().to_string());
                        ui.end_row();

                        ui.label(self.text(T::BackupRoot));
                        ui.monospace(
                            config::expanded_backup_root(&self.config)
                                .display()
                                .to_string(),
                        );
                        ui.end_row();

                        ui.label(self.text(T::GameBackupDir));
                        ui.monospace(
                            backup::game_backup_dir(&self.config, &game)
                                .display()
                                .to_string(),
                        );
                        ui.end_row();

                        ui.label(self.text(T::BackupCount));
                        ui.label(self.backups.len().to_string());
                        ui.end_row();

                        ui.label(self.text(T::BackupStorageMode));
                        ui.label(storage_mode_label(
                            self.language(),
                            game.backup_storage_mode,
                        ));
                        ui.end_row();

                        ui.label(self.text(T::AutoBackup));
                        ui.label(if game.auto_backup.enabled {
                            format_auto_backup_interval(self.language(), &game.auto_backup)
                        } else {
                            "-".to_owned()
                        });
                        ui.end_row();

                        ui.label(self.text(T::NextAutoBackup));
                        ui.label(format_next_auto_backup(&game.auto_backup));
                        ui.end_row();
                    });

                ui.horizontal_wrapped(|ui| {
                    if ui.button(self.text(T::ChooseBackupRoot)).clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.set_backup_root(path);
                        }
                    }
                    if ui.button(self.text(T::OpenSaveDir)).clicked() {
                        self.open_path(expand_path(&game.save_path));
                    }
                    if ui.button(self.text(T::OpenBackupDir)).clicked() {
                        match backup::ensure_game_backup_dir(&self.config, &game) {
                            Ok(path) => self.open_path(path),
                            Err(err) => self.set_error(err),
                        }
                    }
                    if ui.button(self.text(T::Refresh)).clicked() {
                        self.refresh_backups();
                    }
                });

                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(self.text(T::BackupLabel));
                    let backup_hint = self.text(T::BackupHint);
                    ui.add_sized(
                        [260.0, 24.0],
                        egui::TextEdit::singleline(&mut self.backup_label).hint_text(backup_hint),
                    );
                    if ui.button(self.text(T::BackupNow)).clicked() {
                        self.start_backup(false);
                    }
                    let single_selected_backup = self.single_selected_backup_path();
                    let selected_backup_count = self.selected_backup_paths.len();
                    let selection_mode_text = if self.backup_delete_selection_mode {
                        self.text(T::ClearCurrentBackupSelection)
                    } else {
                        self.text(T::SelectCurrentBackup)
                    };
                    if ui
                        .add_enabled(
                            !self.backups.is_empty(),
                            egui::Button::new(selection_mode_text),
                        )
                        .clicked()
                    {
                        self.toggle_backup_delete_selection_mode();
                    }
                    let selection_button_text = if self.all_backups_selected_for_delete() {
                        self.text(T::ClearBackupSelection)
                    } else {
                        self.text(T::SelectAllBackups)
                    };
                    if ui
                        .add_enabled(
                            !self.backups.is_empty(),
                            egui::Button::new(selection_button_text),
                        )
                        .clicked()
                    {
                        if self.all_backups_selected_for_delete() {
                            self.clear_backup_delete_selection();
                        } else {
                            self.select_all_backups_for_delete();
                        }
                    }
                    if ui
                        .add_enabled(
                            single_selected_backup.is_some(),
                            egui::Button::new(self.text(T::RestoreSelected)),
                        )
                        .clicked()
                    {
                        if let Some(path) = single_selected_backup {
                            self.confirm_action =
                                Some(ConfirmAction::RestoreBackup { backup_path: path });
                        }
                    }
                    let delete_text = if selected_backup_count > 1 {
                        match self.language() {
                            crate::models::Language::ZhCn => {
                                format!("删除选中备份 ({selected_backup_count})")
                            }
                            crate::models::Language::EnUs => {
                                format!("Delete Selected ({selected_backup_count})")
                            }
                        }
                    } else {
                        self.text(T::DeleteSelected).to_owned()
                    };
                    if ui
                        .add_enabled(selected_backup_count > 0, egui::Button::new(delete_text))
                        .clicked()
                    {
                        let backup_paths = self.selected_backup_paths.iter().cloned().collect();
                        self.confirm_action = Some(ConfirmAction::DeleteBackups { backup_paths });
                    }
                });

                ui.separator();
                ui.heading(self.text(T::BackupHistory));
                self.draw_backup_history(ui);
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(120.0);
                    ui.heading(self.text(T::NeedAddGameTitle));
                    ui.label(self.text(T::NeedAddGameBody));
                    ui.label(format!(
                        "{}: {}",
                        self.text(T::CurrentBackupRoot),
                        config::expanded_backup_root(&self.config).display()
                    ));
                    if ui.button(self.text(T::ChooseBackupRoot)).clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.set_backup_root(path);
                        }
                    }
                    if ui.button(self.text(T::AddGame)).clicked() {
                        self.open_add_game_dialog();
                    }
                });
            }
        });
    }

    pub(crate) fn draw_backup_history(&mut self, ui: &mut egui::Ui) {
        let row_height = 28.0;
        egui::ScrollArea::both()
            .id_source("backup_history_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut toggled_paths = Vec::new();
                let mut switched_path = None;
                egui::Grid::new("backup_history")
                    .striped(true)
                    .num_columns(6)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        centered_header(ui, 48.0, self.text(T::Select));
                        centered_header(ui, 260.0, self.text(T::Label));
                        centered_header(ui, 136.0, self.text(T::CreatedAt));
                        centered_header(ui, 48.0, self.text(T::FileCount));
                        centered_header(ui, 58.0, self.text(T::Size));
                        centered_header(ui, 220.0, self.text(T::Type));
                        ui.end_row();

                        if self.backups.is_empty() {
                            ui.label("-");
                            ui.label(self.text(T::NoBackups));
                            ui.label("-");
                            ui.label("-");
                            ui.label("-");
                            ui.label("-");
                            ui.end_row();
                            return;
                        }

                        let backups = self.backups.clone();
                        for backup in &backups {
                            let delete_selected = self.selected_backup_paths.contains(&backup.path);
                            let is_current =
                                self.selected_backup_path.as_ref() == Some(&backup.path);
                            let response = backup_history_select_box(
                                ui,
                                self.backup_delete_selection_mode,
                                is_current,
                                delete_selected,
                            )
                            .on_hover_text(format!(
                                "Game: {}\nID: {}\nPath: {}",
                                backup.game_name,
                                backup.game_id,
                                backup.path.display()
                            ));
                            if response.clicked() {
                                if self.backup_delete_selection_mode {
                                    toggled_paths.push((backup.path.clone(), !delete_selected));
                                } else {
                                    switched_path = Some(backup.path.clone());
                                }
                            }

                            let label_text =
                                backup_label_text(self.language(), backup, self.text(T::NoLabel));
                            centered_cell_label(ui, 260.0, row_height, label_text.as_str())
                                .on_hover_text(label_text);
                            let created_at =
                                backup.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
                            centered_cell_label(ui, 136.0, row_height, created_at.as_str());
                            centered_cell_label(
                                ui,
                                48.0,
                                row_height,
                                backup.file_count.to_string().as_str(),
                            );
                            let size_text = format_size(backup.total_size);
                            centered_cell_label(ui, 58.0, row_height, size_text.as_str());
                            let backup_type = if backup.is_pre_restore_backup {
                                self.text(T::PreRestoreBackup)
                            } else {
                                self.text(T::NormalBackup)
                            };
                            let type_text = format!(
                                "{} / {}",
                                backup_type,
                                storage_kind_label(
                                    self.language(),
                                    backup.storage_kind,
                                    backup.incremental_kind
                                )
                            );
                            centered_cell_label(ui, 220.0, row_height, type_text.as_str());
                            ui.end_row();
                        }
                    });
                if let Some(path) = switched_path {
                    self.select_single_backup(path);
                }
                for (path, selected) in toggled_paths {
                    self.set_backup_delete_selected(path, selected);
                }
            });
    }
}

fn backup_label_text(language: Language, backup: &BackupEntry, no_label: &str) -> String {
    if backup.is_pre_restore_backup {
        return match language {
            Language::ZhCn => "恢复前自动备份".to_owned(),
            Language::EnUs => "Pre-restore automatic backup".to_owned(),
        };
    }

    match backup.label.as_deref() {
        Some("Automatic backup") => match language {
            Language::ZhCn => "自动备份".to_owned(),
            Language::EnUs => "Automatic backup".to_owned(),
        },
        Some(label) => label.to_owned(),
        None => no_label.to_owned(),
    }
}

fn format_auto_backup_interval(language: Language, auto_backup: &AutoBackupConfig) -> String {
    let value = auto_backup.display_interval_value();
    match (language, auto_backup.display_interval_unit()) {
        (Language::ZhCn, AutoBackupIntervalUnit::Minutes) => format!("{value} 分钟"),
        (Language::ZhCn, AutoBackupIntervalUnit::Hours) => format!("{value} 小时"),
        (Language::EnUs, AutoBackupIntervalUnit::Minutes) => {
            if value == 1 {
                "1 min".to_owned()
            } else {
                format!("{value} mins")
            }
        }
        (Language::EnUs, AutoBackupIntervalUnit::Hours) => {
            if value == 1 {
                "1 h".to_owned()
            } else {
                format!("{value} h")
            }
        }
    }
}

fn format_next_auto_backup(auto_backup: &AutoBackupConfig) -> String {
    if !auto_backup.enabled {
        return "-".to_owned();
    }
    let Some(next) = &auto_backup.next_auto_backup_at else {
        return "-".to_owned();
    };
    DateTime::parse_from_rfc3339(next)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| "-".to_owned())
}

fn backup_history_select_box(
    ui: &mut egui::Ui,
    selection_mode: bool,
    current: bool,
    delete_selected: bool,
) -> egui::Response {
    let visuals = ui.visuals();
    let active = if selection_mode {
        delete_selected
    } else {
        current
    };
    let (fill, stroke) = if active {
        (
            egui::Color32::from_rgba_unmultiplied(
                visuals.selection.bg_fill.r(),
                visuals.selection.bg_fill.g(),
                visuals.selection.bg_fill.b(),
                if selection_mode { 118 } else { 52 },
            ),
            egui::Stroke::new(2.0, visuals.selection.stroke.color),
        )
    } else {
        (
            visuals.widgets.inactive.bg_fill,
            egui::Stroke::new(1.0, visuals.widgets.inactive.bg_stroke.color),
        )
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(48.0, 30.0), egui::Sense::click());
    let box_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(28.0, 22.0));
    ui.painter().rect_filled(box_rect, 3.0, fill);
    ui.painter().rect_stroke(box_rect, 3.0, stroke);
    response
}

fn centered_header(ui: &mut egui::Ui, width: f32, text: &str) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 22.0),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.strong(text);
        },
    );
}

fn centered_cell_label(ui: &mut egui::Ui, width: f32, height: f32, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    response
}

fn storage_kind_label(
    language: Language,
    kind: BackupStorageKind,
    incremental_kind: Option<IncrementalBackupKind>,
) -> &'static str {
    match (language, kind, incremental_kind) {
        (
            crate::models::Language::ZhCn,
            BackupStorageKind::Incremental,
            Some(IncrementalBackupKind::Full),
        ) => "全量",
        (crate::models::Language::ZhCn, BackupStorageKind::Incremental, _) => "增量",
        (crate::models::Language::ZhCn, BackupStorageKind::Zip, _) => "ZIP 压缩",
        (crate::models::Language::ZhCn, BackupStorageKind::LegacyDirectory, _) => "旧版完整复制",
        (
            crate::models::Language::EnUs,
            BackupStorageKind::Incremental,
            Some(IncrementalBackupKind::Full),
        ) => "Full",
        (crate::models::Language::EnUs, BackupStorageKind::Incremental, _) => "Incremental",
        (crate::models::Language::EnUs, BackupStorageKind::Zip, _) => "ZIP",
        (crate::models::Language::EnUs, BackupStorageKind::LegacyDirectory, _) => "Legacy",
    }
}

fn storage_mode_label(language: crate::models::Language, mode: BackupStorageMode) -> &'static str {
    match language {
        crate::models::Language::ZhCn => match mode {
            BackupStorageMode::Incremental => "增量备份",
            BackupStorageMode::Zip => "ZIP 压缩备份",
        },
        crate::models::Language::EnUs => match mode {
            BackupStorageMode::Incremental => "Incremental",
            BackupStorageMode::Zip => "ZIP compressed",
        },
    }
}
