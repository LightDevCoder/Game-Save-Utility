use crate::app::{
    status_color, ConfirmAction, GameSaveApp, HelpWindowState, SteamScanAction,
    SteamScanDialogState,
};
use crate::help::{self, HelpLineKind};
use crate::i18n::{t, Text as T};
use crate::models::{CloseBehavior, Language};
use crate::presets;
use eframe::egui;

impl GameSaveApp {
    pub(crate) fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(self.text(T::Status));
                let color = status_color(self.status.kind, ui.visuals());
                ui.colored_label(color, &self.status.text);
            });
        });
    }

    pub(crate) fn draw_game_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_game_dialog {
            return;
        }

        let title = if self.editing_game_id.is_some() {
            self.text(T::EditGame)
        } else {
            self.text(T::AddGame)
        };

        let mut open = self.show_game_dialog;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    let mut chosen_preset = None;
                    egui::ComboBox::from_label(self.text(T::PresetGame))
                        .selected_text(
                            self.game_form
                                .preset_index
                                .and_then(|index| self.presets.get(index))
                                .map(|preset| preset.name.as_str())
                                .unwrap_or(self.text(T::NoPreset)),
                        )
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    self.game_form.preset_index.is_none(),
                                    self.text(T::NoPreset),
                                )
                                .clicked()
                            {
                                chosen_preset = Some(None);
                            }
                            for (index, preset) in self.presets.iter().enumerate() {
                                if ui
                                    .selectable_label(
                                        self.game_form.preset_index == Some(index),
                                        &preset.name,
                                    )
                                    .clicked()
                                {
                                    chosen_preset = Some(Some(index));
                                }
                            }
                        });

                    if let Some(selection) = chosen_preset {
                        self.game_form.preset_index = selection;
                        if let Some(index) = selection {
                            if let Some(preset) = self.presets.get(index) {
                                self.game_form.name = preset.name.clone();
                                if let Some(path) = presets::first_expanded_path(preset) {
                                    self.game_form.save_path = path.to_string_lossy().to_string();
                                }
                            }
                        }
                    }

                    if let Some(index) = self.game_form.preset_index {
                        if let Some(note) = self
                            .presets
                            .get(index)
                            .and_then(|preset| preset.note.as_ref())
                        {
                            ui.label(note);
                        }
                    }

                    ui.separator();
                    ui.label(self.text(T::GameName));
                    ui.text_edit_singleline(&mut self.game_form.name);

                    ui.label(self.text(T::SaveDir));
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [390.0, 24.0],
                            egui::TextEdit::singleline(&mut self.game_form.save_path),
                        );
                        if ui.button(self.text(T::Browse)).clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.game_form.save_path = path.to_string_lossy().to_string();
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        let auto_cleanup_text = self.text(T::EnableAutoCleanup);
                        ui.checkbox(&mut self.game_form.auto_cleanup_enabled, auto_cleanup_text);
                        ui.label(self.text(T::MaxBackups));
                        let unlimited_hint = self.text(T::UnlimitedHint);
                        ui.add_sized(
                            [80.0, 24.0],
                            egui::TextEdit::singleline(&mut self.game_form.max_backups)
                                .hint_text(unlimited_hint),
                        );
                    });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(self.text(T::Save)).clicked() {
                            self.submit_game_form();
                        }
                        if ui.button(self.text(T::Cancel)).clicked() {
                            self.show_game_dialog = false;
                        }
                    });
                });
            });

        self.show_game_dialog = open && self.show_game_dialog;
    }

    pub(crate) fn draw_steam_scan_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_steam_scan_dialog {
            return;
        }

        let Some(state_ref) = self.steam_scan_state.clone() else {
            self.show_steam_scan_dialog = false;
            return;
        };
        let language = self.language();

        let mut builder = egui::ViewportBuilder::default()
            .with_title(t(language, T::SteamScanTitle))
            .with_inner_size([1040.0, 660.0])
            .with_min_inner_size([820.0, 520.0])
            .with_resizable(true)
            .with_decorations(true);
        if let Some(icon) = &self.app_icon {
            builder = builder.with_icon(icon.clone());
        }

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of("steam_scan_results"),
            builder,
            move |ctx, class| {
                if ctx.input(|input| input.viewport().close_requested()) {
                    if let Ok(mut state) = state_ref.lock() {
                        state.open = false;
                    }
                    ctx.request_repaint_of(ctx.parent_viewport_id());
                    return;
                }

                let Ok(mut state) = state_ref.lock() else {
                    return;
                };

                if class == egui::ViewportClass::Embedded {
                    let mut open = state.open;
                    egui::Window::new(t(language, T::SteamScanTitle))
                        .open(&mut open)
                        .collapsible(false)
                        .resizable(true)
                        .default_size([1040.0, 660.0])
                        .show(ctx, |ui| {
                            Self::draw_steam_scan_embedded_contents(ctx, ui, &mut state, language);
                        });
                    if !open {
                        state.open = false;
                        state.pending_action = Some(SteamScanAction::Close);
                        ctx.request_repaint_of(ctx.parent_viewport_id());
                    }
                } else {
                    Self::draw_steam_scan_state_contents(ctx, &mut state, language);
                }
            },
        );
    }

    fn draw_steam_scan_state_contents(
        ctx: &egui::Context,
        state: &mut SteamScanDialogState,
        language: Language,
    ) {
        let mut select_app_id = None;

        egui::TopBottomPanel::bottom("steam_scan_actions")
            .resizable(false)
            .show(ctx, |ui| {
                Self::draw_steam_scan_actions(ctx, ui, state, language);
            });

        let panel = egui::SidePanel::left("steam_scan_game_list_panel")
            .resizable(true)
            .default_width(state.list_width)
            .width_range(280.0..=560.0)
            .show(ctx, |ui| {
                Self::draw_steam_scan_game_list(ui, state, &mut select_app_id, language);
            });
        state.list_width = panel.response.rect.width();

        if let Some(app_id) = select_app_id {
            state.select_candidate(app_id);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            Self::draw_steam_scan_details(ui, state, language);
        });
    }

    fn draw_steam_scan_embedded_contents(
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        state: &mut SteamScanDialogState,
        language: Language,
    ) {
        let mut select_app_id = None;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(state.list_width.clamp(280.0, 560.0));
                Self::draw_steam_scan_game_list(ui, state, &mut select_app_id, language);
            });
            ui.separator();
            ui.vertical(|ui| {
                Self::draw_steam_scan_details(ui, state, language);
            });
        });

        if let Some(app_id) = select_app_id {
            state.select_candidate(app_id);
        }

        ui.separator();
        Self::draw_steam_scan_actions(ctx, ui, state, language);
    }

    fn draw_steam_scan_game_list(
        ui: &mut egui::Ui,
        state: &SteamScanDialogState,
        select_app_id: &mut Option<String>,
        language: Language,
    ) {
        ui.strong(format!(
            "{} {}",
            t(language, T::GamesCount),
            state.candidates.len()
        ));
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_source("steam_scan_game_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for candidate in &state.candidates {
                    let selected = state
                        .selected_app_id
                        .as_ref()
                        .is_some_and(|app_id| app_id == &candidate.app_id);
                    let suffix = if candidate.save_paths.is_empty() {
                        t(language, T::NoCandidate)
                    } else {
                        t(language, T::HasCandidate)
                    };
                    if ui
                        .selectable_label(selected, format!("{}  ·  {}", candidate.name, suffix))
                        .on_hover_text(format!(
                            "AppID: {}\n{}: {}",
                            candidate.app_id,
                            t(language, T::InstallDir),
                            candidate.install_dir.display()
                        ))
                        .clicked()
                    {
                        *select_app_id = Some(candidate.app_id.clone());
                    }
                }
            });
    }

    fn draw_steam_scan_details(
        ui: &mut egui::Ui,
        state: &mut SteamScanDialogState,
        language: Language,
    ) {
        egui::ScrollArea::vertical()
            .id_source("steam_scan_details")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(candidate) = state.selected_candidate().cloned() {
                    ui.heading(&candidate.name);
                    ui.label(format!("AppID: {}", candidate.app_id));
                    ui.label(t(language, T::InstallDir));
                    ui.monospace(candidate.install_dir.display().to_string());
                    ui.add_space(8.0);

                    ui.strong(t(language, T::CandidateSaveDirs));
                    if candidate.save_paths.is_empty() {
                        ui.label(t(language, T::NoCandidate));
                    }

                    for save_candidate in &candidate.save_paths {
                        let selected = state
                            .selected_save_path
                            .as_ref()
                            .is_some_and(|path| path == &save_candidate.path);
                        let exists = if save_candidate.exists {
                            t(language, T::Available)
                        } else {
                            t(language, T::Missing)
                        };
                        let source =
                            Self::steam_save_source_label(language, &save_candidate.source);
                        if ui
                            .radio(selected, format!("{} · {}", exists, source))
                            .clicked()
                        {
                            state.selected_save_path = Some(save_candidate.path.clone());
                        }
                        ui.monospace(save_candidate.path.display().to_string());
                    }

                    ui.add_space(8.0);
                    if ui.button(t(language, T::ChooseOtherSaveDir)).clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            state.selected_save_path = Some(path);
                        }
                    }

                    if let Some(path) = &state.selected_save_path {
                        ui.label(t(language, T::WillUseDir));
                        ui.monospace(path.display().to_string());
                    }
                } else {
                    ui.label(t(language, T::NoGameSelected));
                }
            });
    }

    fn draw_steam_scan_actions(
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        state: &mut SteamScanDialogState,
        language: Language,
    ) {
        ui.horizontal(|ui| {
            let can_add =
                state.selected_save_path.is_some() && state.selected_candidate().is_some();
            if ui
                .add_enabled(can_add, egui::Button::new(t(language, T::AddSelectedGame)))
                .clicked()
            {
                if let (Some(app_id), Some(save_path)) = (
                    state
                        .selected_candidate()
                        .map(|candidate| candidate.app_id.clone()),
                    state.selected_save_path.clone(),
                ) {
                    state.pending_action = Some(SteamScanAction::AddSelected { app_id, save_path });
                    ctx.request_repaint_of(ctx.parent_viewport_id());
                }
            }
            let can_add_all = state
                .candidates
                .iter()
                .any(|candidate| candidate.recommended_save_path().is_some());
            if ui
                .add_enabled(
                    can_add_all,
                    egui::Button::new(t(language, T::AddAllCandidates)),
                )
                .clicked()
            {
                state.pending_action = Some(SteamScanAction::AddAll);
                ctx.request_repaint_of(ctx.parent_viewport_id());
            }
            if ui.button(t(language, T::Rescan)).clicked() {
                state.pending_action = Some(SteamScanAction::Rescan);
                ctx.request_repaint_of(ctx.parent_viewport_id());
            }
            if ui.button(t(language, T::Close)).clicked() {
                state.open = false;
                state.pending_action = Some(SteamScanAction::Close);
                ctx.request_repaint_of(ctx.parent_viewport_id());
            }
        });
    }

    fn steam_save_source_label<'a>(
        language: Language,
        source: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        match source {
            "Steam Cloud 本地缓存" => {
                std::borrow::Cow::Borrowed(t(language, T::SteamCloudLocalCache))
            }
            "内置常见路径" => std::borrow::Cow::Borrowed(t(language, T::BuiltInCommonPaths)),
            _ => std::borrow::Cow::Borrowed(source),
        }
    }

    pub(crate) fn draw_confirmation_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm_action.clone() else {
            return;
        };

        let title = match action {
            ConfirmAction::BackupEmptySaveDir { .. } => self.text(T::ConfirmEmptyBackupTitle),
            ConfirmAction::RestoreBackup { .. } => self.text(T::ConfirmRestoreTitle),
            ConfirmAction::DeleteBackup { .. } => self.text(T::ConfirmDeleteBackupTitle),
            ConfirmAction::DeleteGame { .. } => self.text(T::ConfirmDeleteGameTitle),
        };

        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .show(ctx, |ui| {
                match &action {
                    ConfirmAction::BackupEmptySaveDir { .. } => {
                        ui.label(self.text(T::ConfirmEmptyBackupBody));
                    }
                    ConfirmAction::RestoreBackup { backup_path } => {
                        ui.label(self.text(T::ConfirmRestoreBody));
                        ui.monospace(backup_path.display().to_string());
                    }
                    ConfirmAction::DeleteBackup { backup_path } => {
                        ui.label(self.text(T::ConfirmDeleteBackupBody));
                        ui.monospace(backup_path.display().to_string());
                    }
                    ConfirmAction::DeleteGame { .. } => {
                        ui.label(self.text(T::ConfirmDeleteGameBody));
                        let delete_backups_text = self.text(T::DeleteBackupsWithGame);
                        ui.checkbox(&mut self.delete_backups_with_game, delete_backups_text);
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(self.text(T::Confirm)).clicked() {
                        let action = self.confirm_action.take();
                        if let Some(action) = action {
                            self.execute_confirmed_action(action);
                        }
                    }
                    if ui.button(self.text(T::Cancel)).clicked() {
                        self.confirm_action = None;
                    }
                });
            });
    }

    pub(crate) fn draw_close_behavior_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_close_behavior_dialog {
            return;
        }

        egui::Window::new(self.text(T::CloseBehaviorTitle))
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(self.text(T::CloseBehaviorQuestion));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(self.text(T::MinimizeToTray)).clicked() {
                        self.choose_close_behavior(CloseBehavior::MinimizeToTray, ctx);
                    }
                    if ui.button(self.text(T::ExitApp)).clicked() {
                        self.choose_close_behavior(CloseBehavior::Exit, ctx);
                    }
                    if ui.button(self.text(T::Cancel)).clicked() {
                        self.show_close_behavior_dialog = false;
                    }
                });
            });
    }

    pub(crate) fn draw_help_window(&mut self, ctx: &egui::Context) {
        let Some(state_ref) = self.help_window_state.clone() else {
            return;
        };
        let should_close = match state_ref.lock() {
            Ok(state) => !state.open,
            Err(_) => true,
        };
        if should_close {
            self.help_window_state = None;
            return;
        }
        let language = self.language();
        let mut builder = egui::ViewportBuilder::default()
            .with_title(t(language, T::HelpWindowTitle))
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([760.0, 500.0])
            .with_resizable(true)
            .with_decorations(true);
        if let Some(icon) = &self.app_icon {
            builder = builder.with_icon(icon.clone());
        }

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of("help_window"),
            builder,
            move |ctx, class| {
                if ctx.input(|input| input.viewport().close_requested()) {
                    if let Ok(mut state) = state_ref.lock() {
                        state.open = false;
                    }
                    ctx.request_repaint_of(ctx.parent_viewport_id());
                    return;
                }

                let Ok(mut state) = state_ref.lock() else {
                    return;
                };

                if class == egui::ViewportClass::Embedded {
                    let mut open = state.open;
                    egui::Window::new(t(language, T::HelpWindowTitle))
                        .open(&mut open)
                        .resizable(true)
                        .default_size([900.0, 620.0])
                        .min_width(760.0)
                        .min_height(500.0)
                        .show(ctx, |ui| {
                            Self::draw_help_contents_ui(ui, &mut state, language);
                        });
                    if !open {
                        state.open = false;
                        ctx.request_repaint_of(ctx.parent_viewport_id());
                    }
                } else {
                    Self::draw_help_state_contents(ctx, &mut state, language);
                }
            },
        );
    }

    fn draw_help_state_contents(
        ctx: &egui::Context,
        state: &mut HelpWindowState,
        language: Language,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            Self::draw_help_contents_ui(ui, state, language);
        });
    }

    fn draw_help_contents_ui(ui: &mut egui::Ui, state: &mut HelpWindowState, language: Language) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(!state.search_mode, t(language, T::HelpContents))
                .clicked()
            {
                state.search_mode = false;
            }
            if ui
                .selectable_label(state.search_mode, t(language, T::HelpSearch))
                .clicked()
            {
                state.search_mode = true;
            }
        });
        ui.separator();

        let nav_width = 260.0;
        let available = ui.available_rect_before_wrap();
        let nav_rect = egui::Rect::from_min_max(
            available.min,
            egui::pos2(
                (available.min.x + nav_width).min(available.max.x),
                available.max.y,
            ),
        );
        let detail_rect = egui::Rect::from_min_max(
            egui::pos2(nav_rect.max.x + 10.0, available.min.y),
            available.max,
        );

        ui.allocate_ui_at_rect(nav_rect, |ui| {
            if state.search_mode {
                ui.add_sized(
                    [ui.available_width(), 24.0],
                    egui::TextEdit::singleline(&mut state.search)
                        .hint_text(t(language, T::HelpSearchHint)),
                );
                ui.separator();
            }

            let query = state.search.clone();
            let mut any_topic = false;
            egui::ScrollArea::vertical()
                .id_source("help_topic_list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for topic in help::topics() {
                        if state.search_mode && !topic.contains(language, &query) {
                            continue;
                        }
                        any_topic = true;
                        let selected = state.selected_topic_id == topic.id;
                        if ui
                            .selectable_label(selected, format!("+ {}", topic.title(language)))
                            .clicked()
                        {
                            state.selected_topic_id = topic.id.to_owned();
                        }
                    }
                    if !any_topic {
                        ui.label(t(language, T::HelpNoResults));
                    }
                });
        });

        ui.painter().line_segment(
            [
                egui::pos2(nav_rect.max.x + 4.0, available.min.y),
                egui::pos2(nav_rect.max.x + 4.0, available.max.y),
            ],
            ui.visuals().widgets.noninteractive.bg_stroke,
        );

        ui.allocate_ui_at_rect(detail_rect, |ui| {
            let topic = help::topic_by_id(&state.selected_topic_id);
            egui::ScrollArea::vertical()
                .id_source("help_topic_detail")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.heading(
                        egui::RichText::new(topic.title(language))
                            .color(egui::Color32::from_rgb(0, 100, 220))
                            .strong(),
                    );
                    ui.separator();
                    for line in topic.lines {
                        let text = match language {
                            Language::ZhCn => line.zh,
                            Language::EnUs => line.en,
                        };
                        match line.kind {
                            HelpLineKind::Heading => {
                                ui.add_space(4.0);
                                ui.strong(text);
                            }
                            HelpLineKind::Paragraph => {
                                ui.label(text);
                            }
                            HelpLineKind::Bullet => {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("-");
                                    ui.label(text);
                                });
                            }
                        }
                        ui.add_space(4.0);
                    }
                });
        });
    }

    fn execute_confirmed_action(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::BackupEmptySaveDir { game_id } => {
                if self.selected_game_id.as_deref() == Some(game_id.as_str()) {
                    self.start_backup(true);
                }
            }
            ConfirmAction::RestoreBackup { backup_path } => {
                self.restore_selected_backup(&backup_path);
            }
            ConfirmAction::DeleteBackup { backup_path } => {
                self.delete_selected_backup(&backup_path);
            }
            ConfirmAction::DeleteGame { game_id } => {
                self.delete_game(&game_id);
            }
        }
    }
}
