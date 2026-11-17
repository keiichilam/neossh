use neo_ssh::preferences::Preferences;

pub struct Dialog {
    pub preferences: Preferences,
    focus_initial: bool,
    theme_popup_open: bool,
}

impl Dialog {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            preferences,
            focus_initial: true,
            theme_popup_open: false,
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> (bool, bool) {
        let mut restore_theme_focus = false;
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if ctx.memory(|memory| memory.any_popup_open()) {
                ctx.memory_mut(|memory| memory.close_popup());
                restore_theme_focus = true;
            } else {
                return (false, false);
            }
        }
        let mut open = true;
        let mut save = false;
        egui::Window::new("Preferences")
            .collapsible(false)
            .resizable(false)
            .default_width(340.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((ctx.screen_rect().height() - 140.0).max(70.0))
                    .show(ui, |ui| {
                        let zoom = ui.add(
                            egui::Slider::new(&mut self.preferences.zoom, 0.75..=2.0)
                                .step_by(0.05)
                                .text("Interface zoom"),
                        );
                        if self.focus_initial {
                            zoom.request_focus();
                            self.focus_initial = false;
                        }
                        ui.add(
                            egui::Slider::new(&mut self.preferences.font_size, 10.0..=28.0)
                                .step_by(1.0)
                                .text("Terminal text size"),
                        );
                        let focus_choice = !self.theme_popup_open;
                        let mut selected_choice = false;
                        let theme = egui::ComboBox::from_label("Terminal theme")
                            .selected_text(&self.preferences.theme)
                            .show_ui(ui, |ui| {
                                for theme in neo_ssh::preferences::THEMES {
                                    let choice = ui.selectable_value(
                                        &mut self.preferences.theme,
                                        theme.into(),
                                        theme,
                                    );
                                    if focus_choice && self.preferences.theme == theme {
                                        choice.request_focus();
                                    }
                                    if choice.clicked() {
                                        selected_choice = true;
                                        ui.memory_mut(|memory| memory.close_popup());
                                    }
                                }
                            });
                        self.theme_popup_open = egui::ComboBox::is_open(ctx, theme.response.id);
                        if restore_theme_focus || selected_choice {
                            theme.response.request_focus();
                        }
                        if neo_ssh::preferences::known_theme(&self.preferences.theme).is_none() {
                            ui.small(
                        "This saved theme uses Midnight colors until you select a supported theme.",
                    );
                        }
                        ui.separator();
                        ui.label("For new connections:");
                        ui.checkbox(
                            &mut self.preferences.keep_alive,
                            "Send SSH keep-alive requests",
                        );
                        ui.horizontal(|ui| {
                            ui.checkbox(
                                &mut self.preferences.scrollback,
                                "Keep terminal scrollback",
                            );
                            ui.add_enabled(
                                self.preferences.scrollback,
                                egui::DragValue::new(&mut self.preferences.scrollback_lines)
                                    .range(
                                        neo_ssh::preferences::MIN_SCROLLBACK_LINES
                                            ..=neo_ssh::preferences::MAX_SCROLLBACK_LINES,
                                    )
                                    .speed(100.0)
                                    .suffix(" lines"),
                            )
                            .on_hover_text(
                                "Lines kept per tab. Each 10,000 lines of a wide terminal use about 100 MB.",
                            );
                        });
                    });
                ui.horizontal(|ui| {
                    save = ui.button("Save preferences").clicked();
                    if ui.button("Cancel").clicked() {
                        open = false;
                    }
                });
            });
        (open, save)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes_theme_choices_before_cancelling_preferences() {
        let ctx = egui::Context::default();
        let mut dialog = Dialog::new(Preferences::load(&Default::default()));
        let frame = |dialog: &mut Dialog, key: Option<egui::Key>| {
            let mut result = (false, false);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events: key
                    .into_iter()
                    .flat_map(|key| {
                        [true, false].map(|pressed| egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        })
                    })
                    .collect(),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| result = dialog.show(ctx));
            result
        };
        frame(&mut dialog, None);
        // Seed an unsaved edit; slider editing is covered separately below.
        dialog.preferences.zoom += 0.05;
        let edited_zoom = dialog.preferences.zoom;
        for _ in 0..4 {
            frame(&mut dialog, Some(egui::Key::Tab));
        }
        let theme_focus = ctx.memory(|memory| memory.focused());
        frame(&mut dialog, Some(egui::Key::Enter));
        assert!(
            ctx.memory(|memory| memory.any_popup_open()),
            "Theme choices did not open"
        );
        assert_eq!(frame(&mut dialog, Some(egui::Key::Escape)), (true, false));
        assert!(!ctx.memory(|memory| memory.any_popup_open()));
        assert_eq!(ctx.memory(|memory| memory.focused()), theme_focus);
        assert_eq!(dialog.preferences.zoom, edited_zoom);
        frame(&mut dialog, Some(egui::Key::Enter));
        assert!(ctx.memory(|memory| memory.any_popup_open()));
        assert!(ctx.memory(|memory| memory.focused()).is_some());
        frame(&mut dialog, Some(egui::Key::Tab));
        frame(&mut dialog, Some(egui::Key::Enter));
        assert_eq!(dialog.preferences.theme, "Forest");
        assert!(!ctx.memory(|memory| memory.any_popup_open()));
        assert_eq!(ctx.memory(|memory| memory.focused()), theme_focus);
        assert_eq!(frame(&mut dialog, Some(egui::Key::Escape)), (false, false));
    }

    #[test]
    fn opens_with_keyboard_focus_and_escape_cancels_without_saving() {
        let ctx = egui::Context::default();
        let mut dialog = Dialog::new(Preferences::load(&Default::default()));
        let mut result = (false, false);
        let _ = ctx.run(Default::default(), |ctx| result = dialog.show(ctx));
        assert_eq!(result, (true, false));
        assert!(ctx.memory(|memory| memory.focused().is_some()));
        let zoom = dialog.preferences.zoom;
        let key = |key| egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(key(egui::Key::ArrowRight), |ctx| result = dialog.show(ctx));
        assert!(dialog.preferences.zoom > zoom);
        let _ = ctx.run(key(egui::Key::Escape), |ctx| {
            result = dialog.show(ctx);
            assert!(!ctx.input(|input| input.key_pressed(egui::Key::Escape)));
        });
        assert_eq!(result, (false, false));
    }
}
