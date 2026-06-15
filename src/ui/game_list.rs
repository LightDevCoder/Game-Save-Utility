use crate::app::{status_color, ConfirmAction, GameSaveApp};
use crate::i18n::Text as T;
use eframe::egui;

impl GameSaveApp {
    pub(crate) fn draw_game_list(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("game_list")
            .resizable(true)
            .default_width(230.0)
            .width_range(190.0..=320.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    ui.heading(self.text(T::GameList));
                });
                ui.separator();

                let available = ui.available_rect_before_wrap();
                let footer_height = 196.0;
                let footer_top = (available.max.y - footer_height)
                    .max(available.min.y + 80.0)
                    .min(available.max.y);
                let list_rect = egui::Rect::from_min_max(
                    available.min,
                    egui::pos2(available.max.x, footer_top),
                );
                let footer_rect = egui::Rect::from_min_max(
                    egui::pos2(available.min.x, footer_top),
                    available.max,
                );

                ui.allocate_ui_at_rect(list_rect, |ui| {
                    egui::ScrollArea::vertical()
                        .id_source("game_list_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space(8.0);
                            if self.config.games.is_empty() {
                                ui.label(self.text(T::NoGames));
                            } else {
                                let games = self.config.games.clone();
                                for game in games {
                                    let selected = self
                                        .selected_game_id
                                        .as_ref()
                                        .is_some_and(|id| id == &game.id);
                                    if ui.selectable_label(selected, &game.name).clicked() {
                                        self.select_game(game.id);
                                    }
                                }
                            }
                        });
                });

                ui.allocate_ui_at_rect(footer_rect, |ui| {
                    ui.separator();
                    let color = status_color(self.status.kind, ui.visuals());
                    egui::ScrollArea::vertical()
                        .id_source("game_list_status")
                        .max_height(60.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.colored_label(color, &self.status.text);
                        });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let enabled = self.selected_game_id.is_some();
                        if ui
                            .add_enabled(enabled, egui::Button::new(self.text(T::Edit)))
                            .clicked()
                        {
                            self.open_edit_game_dialog();
                        }
                        if ui
                            .add_enabled(enabled, egui::Button::new(self.text(T::Delete)))
                            .clicked()
                        {
                            if let Some(game_id) = self.selected_game_id.clone() {
                                self.delete_backups_with_game = false;
                                self.confirm_action = Some(ConfirmAction::DeleteGame { game_id });
                            }
                        }
                    });
                    if ui
                        .add_sized([122.0, 28.0], egui::Button::new(self.text(T::ScanSteam)))
                        .clicked()
                    {
                        self.open_steam_scan_dialog();
                    }
                    if ui
                        .add_sized([92.0, 28.0], egui::Button::new(self.text(T::AddGame)))
                        .clicked()
                    {
                        self.open_add_game_dialog();
                    }
                });
            });
    }
}
