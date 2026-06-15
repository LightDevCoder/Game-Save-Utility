use crate::app::{ConfirmAction, GameSaveApp};
use crate::backup;
use crate::config;
use crate::fs_utils::{expand_path, format_size};
use crate::i18n::Text as T;
use crate::models::{CloseBehavior, StatusMessage};
use eframe::egui;

impl GameSaveApp {
    pub(crate) fn draw_main_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 38.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(self.text(T::CloseButton));
                    let mut behavior = self.config.settings.close_behavior;
                    egui::ComboBox::from_id_source("close_behavior_setting")
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
                    ui.label(self.text(T::DataDir));
                    let reserved_button_width = 276.0;
                    let path_width =
                        (ui.available_width() - reserved_button_width).clamp(160.0, 360.0);
                    match config::app_data_dir() {
                        Ok(path) => {
                            ui.add_sized(
                                [path_width, 24.0],
                                egui::Label::new(
                                    egui::RichText::new(path.display().to_string()).monospace(),
                                )
                                .truncate(true),
                            )
                            .on_hover_text(path.display().to_string());
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
                    if ui.button(self.text(T::MoveData)).clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.change_app_data_dir(path);
                        }
                    }
                    ui.separator();
                    if ui.button(self.text(T::Docs)).clicked() {
                        self.open_user_guide();
                    }
                    if ui.button(self.text(T::SwitchLanguage)).clicked() {
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
                    });

                ui.horizontal(|ui| {
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
                ui.horizontal(|ui| {
                    ui.label(self.text(T::BackupLabel));
                    let backup_hint = self.text(T::BackupHint);
                    ui.add_sized(
                        [260.0, 24.0],
                        egui::TextEdit::singleline(&mut self.backup_label).hint_text(backup_hint),
                    );
                    if ui.button(self.text(T::BackupNow)).clicked() {
                        self.start_backup(false);
                    }
                    let has_backup = self.selected_backup_path.is_some();
                    if ui
                        .add_enabled(has_backup, egui::Button::new(self.text(T::RestoreSelected)))
                        .clicked()
                    {
                        if let Some(path) = self.selected_backup_path.clone() {
                            self.confirm_action =
                                Some(ConfirmAction::RestoreBackup { backup_path: path });
                        }
                    }
                    if ui
                        .add_enabled(has_backup, egui::Button::new(self.text(T::DeleteSelected)))
                        .clicked()
                    {
                        if let Some(path) = self.selected_backup_path.clone() {
                            self.confirm_action =
                                Some(ConfirmAction::DeleteBackup { backup_path: path });
                        }
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
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut clicked_path = None;
                egui::Grid::new("backup_history")
                    .striped(true)
                    .num_columns(6)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong(self.text(T::Select));
                        ui.strong(self.text(T::Label));
                        ui.strong(self.text(T::CreatedAt));
                        ui.strong(self.text(T::FileCount));
                        ui.strong(self.text(T::Size));
                        ui.strong(self.text(T::Type));
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

                        for backup in &self.backups {
                            let selected = self
                                .selected_backup_path
                                .as_ref()
                                .is_some_and(|path| path == &backup.path);
                            if ui
                                .selectable_label(selected, self.text(T::Selected))
                                .on_hover_text(format!(
                                    "Game: {}\nID: {}\nPath: {}",
                                    backup.game_name,
                                    backup.game_id,
                                    backup.path.display()
                                ))
                                .clicked()
                            {
                                clicked_path = Some(backup.path.clone());
                            }

                            ui.add_sized(
                                [180.0, row_height],
                                egui::Label::new(short_text(
                                    backup.label.as_deref().unwrap_or(self.text(T::NoLabel)),
                                    18,
                                )),
                            )
                            .on_hover_text(
                                backup.label.as_deref().unwrap_or(self.text(T::NoLabel)),
                            );
                            ui.label(backup.created_at.format("%Y-%m-%d %H:%M:%S").to_string());
                            ui.label(backup.file_count.to_string());
                            ui.label(format_size(backup.total_size));
                            ui.label(if backup.is_pre_restore_backup {
                                self.text(T::PreRestoreBackup)
                            } else {
                                self.text(T::NormalBackup)
                            });
                            ui.end_row();
                        }
                    });
                if let Some(path) = clicked_path {
                    self.selected_backup_path = Some(path);
                }
            });
    }
}

fn short_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let shortened: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{shortened}...")
    } else {
        shortened
    }
}
