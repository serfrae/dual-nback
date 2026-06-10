//! Application state, input handling, and all five screens.

use std::time::Instant;

use eframe::egui::{
    self, Align2, Button, CentralPanel, Color32, ComboBox, Context, CornerRadius, DragValue,
    FontId, Frame, Grid, Id, Key, Label, Margin, Pos2, Rect, RichText, ScrollArea, Sense, Shape,
    Slider, StrokeKind, pos2, vec2,
};

use crate::audio::Audio;
use crate::config::{AudioSet, Config};
use crate::game::{
    LevelChange, ModalityScore, Outcome, Press, Session, SessionConfig, SessionResult, TickEvent,
    decide_level, position_cell,
};
use crate::stats::{History, SessionRecord};
use crate::theme;

const COUNTDOWN_MS: f32 = 1800.0;
const FLASH_MS: f32 = 650.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Menu,
    Game,
    Results,
    Stats,
    Settings,
}

/// A transient green/red pulse on an input chip.
#[derive(Clone, Copy)]
struct Flash {
    good: bool,
    remaining_ms: f32,
}

struct GameKeys {
    position: bool,
    audio: bool,
    space: bool,
    escape: bool,
}

/// Key presses with auto-repeat filtered out, so holding a key across a
/// trial boundary can't register in the next trial.
fn game_keys(ctx: &Context) -> GameKeys {
    ctx.input(|i| {
        let fresh = |target: Key| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key { key, pressed: true, repeat: false, .. } if *key == target
                )
            })
        };
        GameKeys {
            position: fresh(Key::A),
            audio: fresh(Key::L),
            space: fresh(Key::Space),
            escape: fresh(Key::Escape),
        }
    })
}

fn decay(flash: &mut Option<Flash>, dt_ms: f32) {
    if let Some(f) = flash {
        f.remaining_ms -= dt_ms;
        if f.remaining_ms <= 0.0 {
            *flash = None;
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

fn stimulus_alpha(elapsed_ms: f32, stim_ms: f32) -> f32 {
    if elapsed_ms >= stim_ms {
        0.0
    } else {
        ((stim_ms - elapsed_ms) / 120.0).clamp(0.0, 1.0)
    }
}

pub struct App {
    config: Config,
    config_dirty: bool,
    history: History,
    audio: Audio,
    screen: Screen,
    session: Option<Session>,
    countdown_ms: f32,
    paused: bool,
    last_frame: Instant,
    last_result: Option<(SessionResult, LevelChange, bool)>,
    pos_flash: Option<Flash>,
    aud_flash: Option<Flash>,
    confirm_reset: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let config = Config::load();
        let audio = Audio::new(config.volume);
        Self {
            config,
            config_dirty: false,
            history: History::load(),
            audio,
            screen: Screen::Menu,
            session: None,
            countdown_ms: 0.0,
            paused: false,
            last_frame: Instant::now(),
            last_result: None,
            pos_flash: None,
            aud_flash: None,
            confirm_reset: false,
        }
    }

    fn save_config_if_dirty(&mut self) {
        if self.config_dirty {
            self.config.save();
            self.config_dirty = false;
        }
    }

    fn start_session(&mut self) {
        self.save_config_if_dirty();
        let cfg = SessionConfig {
            n: self.config.n,
            total_trials: self.config.total_trials(),
            trial_ms: self.config.trial_ms as f32,
            stim_ms: self.config.stim_ms.min(self.config.trial_ms) as f32,
        };
        self.session = Some(Session::new(cfg, &mut rand::rng()));
        self.countdown_ms = COUNTDOWN_MS;
        self.paused = false;
        self.pos_flash = None;
        self.aud_flash = None;
        self.screen = Screen::Game;
    }

    fn abort_session(&mut self) {
        self.session = None;
        self.paused = false;
        self.screen = Screen::Menu;
    }

    fn set_level(&mut self, n: usize) {
        let n = n.clamp(1, 15);
        if n != self.config.n {
            self.config.n = n;
            self.config.fallback_streak = 0;
            self.config_dirty = true;
        }
    }

    fn press_position(&mut self) {
        if self.paused || self.countdown_ms > 0.0 {
            return;
        }
        let Some(session) = &mut self.session else { return };
        let press = session.press_position();
        if self.config.feedback
            && let Some(good) = press_feedback(press)
        {
            self.pos_flash = Some(Flash { good, remaining_ms: FLASH_MS });
        }
    }

    fn press_audio(&mut self) {
        if self.paused || self.countdown_ms > 0.0 {
            return;
        }
        let Some(session) = &mut self.session else { return };
        let press = session.press_audio();
        if self.config.feedback
            && let Some(good) = press_feedback(press)
        {
            self.aud_flash = Some(Flash { good, remaining_ms: FLASH_MS });
        }
    }

    fn handle_tick_event(&mut self, event: TickEvent) {
        match event {
            TickEvent::TrialStarted(i) => {
                if let Some(session) = &self.session {
                    let letter = session.trials[i].letter as usize;
                    self.audio.play_stimulus(self.config.audio_set, letter);
                }
            }
            TickEvent::MissedPosition => {
                if self.config.feedback {
                    self.pos_flash = Some(Flash { good: false, remaining_ms: FLASH_MS });
                }
            }
            TickEvent::MissedAudio => {
                if self.config.feedback {
                    self.aud_flash = Some(Flash { good: false, remaining_ms: FLASH_MS });
                }
            }
            TickEvent::SessionEnded => self.finish_session(),
        }
    }

    fn finish_session(&mut self) {
        let Some(session) = self.session.take() else { return };
        let result = session.result();
        let manual = !self.config.adaptive;
        let change = if manual {
            LevelChange {
                outcome: Outcome::Stay,
                n: self.config.n,
                streak: self.config.fallback_streak,
            }
        } else {
            decide_level(
                self.config.n,
                result.score,
                self.config.threshold_advance,
                self.config.threshold_fallback,
                self.config.fallback_sessions,
                self.config.fallback_streak,
            )
        };
        if !manual {
            self.config.n = change.n;
            self.config.fallback_streak = change.streak;
        }
        self.config.save();
        self.config_dirty = false;
        self.history.push(SessionRecord {
            ts: chrono::Local::now(),
            n: result.n,
            position_pct: result.position.percent(),
            audio_pct: result.audio.percent(),
            score: result.score,
            trials: result.trials,
            outcome: change.outcome,
            manual,
        });
        match change.outcome {
            Outcome::Advance => self.audio.play_advance_cue(),
            Outcome::Fallback => self.audio.play_fallback_cue(),
            Outcome::Stay => self.audio.play_end_cue(),
        }
        self.last_result = Some((result, change, manual));
        self.paused = false;
        self.screen = Screen::Results;
    }

    fn update_game(&mut self, ctx: &Context, dt_ms: f32) {
        let keys = game_keys(ctx);
        if keys.escape {
            self.abort_session();
            return;
        }

        let running = self.session.as_ref().is_some_and(|s| !s.finished());
        if keys.space && running {
            self.paused = !self.paused;
        }
        if running && !ctx.input(|i| i.focused) {
            self.paused = true;
        }

        if !self.paused {
            if self.countdown_ms > 0.0 {
                self.countdown_ms -= dt_ms;
                if self.countdown_ms <= 0.0 {
                    self.countdown_ms = 0.0;
                    let event = self.session.as_mut().map(|s| s.start());
                    if let Some(event) = event {
                        self.handle_tick_event(event);
                    }
                }
            } else {
                if keys.position {
                    self.press_position();
                }
                if keys.audio {
                    self.press_audio();
                }
                let mut events = Vec::new();
                if let Some(session) = &mut self.session {
                    session.tick(dt_ms, &mut events);
                }
                for event in events {
                    self.handle_tick_event(event);
                }
            }
            decay(&mut self.pos_flash, dt_ms);
            decay(&mut self.aud_flash, dt_ms);
        }

        if self.screen == Screen::Game && !self.paused {
            ctx.request_repaint();
        }
    }

    // ----- screens ---------------------------------------------------------

    fn draw_game(&mut self, ui: &mut egui::Ui) {
        let Some(session) = &self.session else {
            self.screen = Screen::Menu;
            return;
        };
        let n = session.cfg.n;
        let total = session.total();
        let index = session.index();
        let started = session.started();
        let elapsed = session.elapsed_in_trial();
        let stim_ms = session.cfg.stim_ms;
        let active_cell = position_cell(session.current_trial().position);
        let pos_pressed = session.pos_pressed();
        let aud_pressed = session.aud_pressed();
        let progress = session.progress();
        let countdown = self.countdown_ms;
        let paused = self.paused;
        let pos_flash = self.pos_flash;
        let aud_flash = self.aud_flash;
        let warmup = started && index < n;
        let audio_label =
            if self.config.audio_set == AudioSet::Letters && self.audio.has_letters() {
                "Letter"
            } else {
                "Tone"
            };

        let (pos_clicked, aud_clicked) = CentralPanel::default()
            .frame(Frame::NONE.fill(theme::BG))
            .show_inside(ui, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter().clone();

                // Top bar.
                let top_y = rect.top() + 32.0;
                painter.text(
                    pos2(rect.left() + 36.0, top_y),
                    Align2::LEFT_CENTER,
                    format!("Dual {n}-Back"),
                    FontId::proportional(20.0),
                    theme::TEXT,
                );
                let trial_label = if started {
                    format!("Trial {} / {}", index + 1, total)
                } else {
                    format!("{total} trials")
                };
                painter.text(
                    pos2(rect.right() - 36.0, top_y),
                    Align2::RIGHT_CENTER,
                    trial_label,
                    FontId::proportional(15.0),
                    theme::TEXT_DIM,
                );

                // Grid.
                let bottom_reserved = 168.0;
                let side = (rect.width() - 90.0)
                    .min(rect.height() - bottom_reserved - 96.0)
                    .clamp(230.0, 520.0);
                let grid_center = pos2(
                    rect.center().x,
                    rect.top() + 64.0 + (rect.height() - bottom_reserved - 64.0 - side) / 2.0
                        + side / 2.0,
                );
                let grid = Rect::from_center_size(grid_center, vec2(side, side));
                let gap = (side * 0.03).clamp(6.0, 14.0);
                let cell_size = (side - 2.0 * gap) / 3.0;

                for row in 0..3 {
                    for col in 0..3 {
                        let cell_index = row * 3 + col;
                        let min = pos2(
                            grid.left() + col as f32 * (cell_size + gap),
                            grid.top() + row as f32 * (cell_size + gap),
                        );
                        let cell = Rect::from_min_size(min, vec2(cell_size, cell_size));
                        if cell_index == 4 {
                            let c = cell.center();
                            let arm = cell_size * 0.09;
                            let stroke = theme::stroke(2.0, theme::CELL_STROKE);
                            painter.line_segment(
                                [pos2(c.x - arm, c.y), pos2(c.x + arm, c.y)],
                                stroke,
                            );
                            painter.line_segment(
                                [pos2(c.x, c.y - arm), pos2(c.x, c.y + arm)],
                                stroke,
                            );
                            continue;
                        }
                        painter.rect_filled(cell, CornerRadius::same(14), theme::CELL);
                        painter.rect_stroke(
                            cell,
                            CornerRadius::same(14),
                            theme::stroke(1.0, theme::CELL_STROKE),
                            StrokeKind::Inside,
                        );
                        if started && countdown <= 0.0 && cell_index == active_cell {
                            let alpha = stimulus_alpha(elapsed, stim_ms);
                            if alpha > 0.0 {
                                let pop = (elapsed / 90.0).clamp(0.0, 1.0);
                                let ease = 1.0 - (1.0 - pop) * (1.0 - pop);
                                let inset =
                                    cell_size * 0.085 + (1.0 - ease) * cell_size * 0.06;
                                painter.rect_filled(
                                    cell.shrink(inset),
                                    CornerRadius::same(12),
                                    theme::ACCENT.gamma_multiply(alpha),
                                );
                            }
                        }
                    }
                }

                if warmup && countdown <= 0.0 {
                    painter.text(
                        pos2(grid.center().x, grid.bottom() + 24.0),
                        Align2::CENTER_CENTER,
                        format!("memorize — matches begin at trial {}", n + 1),
                        FontId::proportional(13.0),
                        theme::TEXT_DIM,
                    );
                }

                // Input chips.
                let chip_w = 215.0_f32.min((rect.width() - 110.0) / 2.0);
                let chip_h = 54.0;
                let chip_y = rect.bottom() - 106.0;
                let chip_gap = 18.0;
                let pos_rect = Rect::from_center_size(
                    pos2(rect.center().x - chip_w / 2.0 - chip_gap / 2.0, chip_y),
                    vec2(chip_w, chip_h),
                );
                let aud_rect = Rect::from_center_size(
                    pos2(rect.center().x + chip_w / 2.0 + chip_gap / 2.0, chip_y),
                    vec2(chip_w, chip_h),
                );
                let pos_clicked =
                    draw_chip(ui, pos_rect, "chip-position", "A", "Position", pos_pressed, pos_flash);
                let aud_clicked =
                    draw_chip(ui, aud_rect, "chip-audio", "L", audio_label, aud_pressed, aud_flash);

                // Session progress bar.
                let bar_w = chip_w * 2.0 + chip_gap;
                let bar = Rect::from_center_size(
                    pos2(rect.center().x, rect.bottom() - 58.0),
                    vec2(bar_w, 4.0),
                );
                painter.rect_filled(bar, CornerRadius::same(2), theme::CELL_STROKE);
                if started {
                    let fill = Rect::from_min_size(
                        bar.min,
                        vec2(bar.width() * progress.clamp(0.0, 1.0), bar.height()),
                    );
                    painter.rect_filled(fill, CornerRadius::same(2), theme::ACCENT);
                }

                painter.text(
                    pos2(rect.center().x, rect.bottom() - 30.0),
                    Align2::CENTER_CENTER,
                    "Space — pause      Esc — end session",
                    FontId::proportional(12.5),
                    theme::TEXT_DIM,
                );

                if countdown > 0.0 {
                    let step = (countdown / 600.0).ceil().max(1.0) as i32;
                    painter.text(
                        grid.center(),
                        Align2::CENTER_CENTER,
                        step.to_string(),
                        FontId::proportional(72.0),
                        theme::ACCENT,
                    );
                    painter.text(
                        pos2(grid.center().x, grid.bottom() + 24.0),
                        Align2::CENTER_CENTER,
                        "get ready…",
                        FontId::proportional(14.0),
                        theme::TEXT_DIM,
                    );
                }

                if paused {
                    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_black_alpha(170));
                    painter.text(
                        rect.center() - vec2(0.0, 16.0),
                        Align2::CENTER_CENTER,
                        "Paused",
                        FontId::proportional(34.0),
                        theme::TEXT,
                    );
                    painter.text(
                        rect.center() + vec2(0.0, 24.0),
                        Align2::CENTER_CENTER,
                        "Space — resume · Esc — end session",
                        FontId::proportional(14.0),
                        theme::TEXT_DIM,
                    );
                }

                (pos_clicked, aud_clicked)
            })
            .inner;

        if pos_clicked {
            self.press_position();
        }
        if aud_clicked {
            self.press_audio();
        }
    }

    fn draw_menu(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        let (start_key, dec_key, inc_key, stats_key) = ctx.input(|i| {
            (
                i.key_pressed(Key::Space) || i.key_pressed(Key::Enter),
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::Minus),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::Plus),
                i.key_pressed(Key::S),
            )
        });
        if dec_key {
            self.set_level(self.config.n.saturating_sub(1));
        }
        if inc_key {
            self.set_level(self.config.n + 1);
        }

        let mut start_clicked = false;
        let mut stats_clicked = false;
        let mut settings_clicked = false;
        let mut dec_clicked = false;
        let mut inc_clicked = false;

        CentralPanel::default().frame(Frame::NONE.fill(theme::BG)).show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(((ui.available_height() - 460.0) / 2.0).max(24.0));
                ui.label(RichText::new("DUAL N-BACK").size(46.0).strong());
                ui.add_space(2.0);
                ui.label(
                    RichText::new("working-memory training · inspired by Brain Workshop")
                        .size(14.0)
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(34.0);

                ui.horizontal(|ui| {
                    let row_w = 44.0 + 200.0 + 44.0 + 2.0 * ui.spacing().item_spacing.x;
                    ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
                    if ui.add_sized([44.0, 44.0], Button::new(RichText::new("−").size(20.0))).clicked()
                    {
                        dec_clicked = true;
                    }
                    ui.add_sized(
                        [200.0, 44.0],
                        Label::new(
                            RichText::new(format!("Dual {}-Back", self.config.n))
                                .size(23.0)
                                .strong(),
                        )
                        .selectable(false),
                    );
                    if ui.add_sized([44.0, 44.0], Button::new(RichText::new("+").size(20.0))).clicked()
                    {
                        inc_clicked = true;
                    }
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - 320.0) / 2.0).max(0.0));
                    if ui
                        .checkbox(
                            &mut self.config.adaptive,
                            RichText::new(format!(
                                "Adaptive level ({}% up · {}% down)",
                                self.config.threshold_advance, self.config.threshold_fallback
                            ))
                            .size(13.5),
                        )
                        .changed()
                    {
                        self.config_dirty = true;
                    }
                });

                ui.add_space(28.0);
                if ui
                    .add(
                        Button::new(
                            RichText::new("Start session")
                                .size(18.0)
                                .strong()
                                .color(Color32::from_rgb(0x0c, 0x12, 0x1f)),
                        )
                        .min_size(vec2(250.0, 54.0))
                        .fill(theme::ACCENT),
                    )
                    .clicked()
                {
                    start_clicked = true;
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "{} trials · {:.1} s each · or press Space",
                        self.config.total_trials(),
                        self.config.trial_ms as f32 / 1000.0
                    ))
                    .size(12.5)
                    .color(theme::TEXT_DIM),
                );

                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    let row_w = 2.0 * 116.0 + ui.spacing().item_spacing.x;
                    ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
                    if ui.add_sized([116.0, 40.0], Button::new("Stats")).clicked() {
                        stats_clicked = true;
                    }
                    if ui.add_sized([116.0, 40.0], Button::new("Settings")).clicked() {
                        settings_clicked = true;
                    }
                });

                ui.add_space(30.0);
                let today = self.history.sessions_today();
                if let Some(best) = self.history.best_level() {
                    ui.label(
                        RichText::new(format!(
                            "{today} session{} today · best level D{best}B",
                            if today == 1 { "" } else { "s" }
                        ))
                        .size(12.5)
                        .color(theme::TEXT_DIM),
                    );
                } else {
                    ui.label(
                        RichText::new(
                            "press A when the position repeats from n back · L for the sound",
                        )
                        .size(12.5)
                        .color(theme::TEXT_DIM),
                    );
                }
            });
        });

        if dec_clicked {
            self.set_level(self.config.n.saturating_sub(1));
        }
        if inc_clicked {
            self.set_level(self.config.n + 1);
        }
        if start_clicked || start_key {
            self.start_session();
        } else if stats_clicked || stats_key {
            self.screen = Screen::Stats;
        } else if settings_clicked {
            self.confirm_reset = false;
            self.screen = Screen::Settings;
        }
    }

    fn draw_results(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        let Some((result, change, manual)) = self.last_result else {
            self.screen = Screen::Menu;
            return;
        };
        let (again_key, menu_key, stats_key) = ctx.input(|i| {
            (
                i.key_pressed(Key::Space) || i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::S),
            )
        });

        let adv = self.config.threshold_advance;
        let fb = self.config.threshold_fallback;
        let mut again_clicked = false;
        let mut menu_clicked = false;
        let mut stats_clicked = false;

        CentralPanel::default().frame(Frame::NONE.fill(theme::BG)).show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(((ui.available_height() - 520.0) / 2.0).max(20.0));
                ui.label(RichText::new("Session complete").size(28.0).strong());
                ui.add_space(6.0);

                match change.outcome {
                    Outcome::Advance => {
                        ui.label(
                            RichText::new(format!(
                                "Level up!  Next session: Dual {}-Back",
                                change.n
                            ))
                            .size(16.0)
                            .color(theme::GREEN),
                        );
                    }
                    Outcome::Fallback => {
                        ui.label(
                            RichText::new(format!(
                                "Stepping back — next session: Dual {}-Back",
                                change.n
                            ))
                            .size(16.0)
                            .color(theme::RED),
                        );
                    }
                    Outcome::Stay if manual => {
                        ui.label(
                            RichText::new(format!(
                                "Manual mode — staying at Dual {}-Back",
                                change.n
                            ))
                            .size(16.0)
                            .color(theme::TEXT_DIM),
                        );
                    }
                    Outcome::Stay => {
                        let strikes = if change.streak > 0 {
                            format!(
                                "  ·  {}/{} strikes below {fb}%",
                                change.streak, self.config.fallback_sessions
                            )
                        } else {
                            String::new()
                        };
                        ui.label(
                            RichText::new(format!(
                                "Keep training at Dual {}-Back{strikes}",
                                change.n
                            ))
                            .size(16.0)
                            .color(theme::TEXT_DIM),
                        );
                    }
                }

                ui.add_space(20.0);
                ui.label(
                    RichText::new(format!("{}%", result.score))
                        .size(64.0)
                        .strong()
                        .color(theme::score_color(result.score, adv, fb)),
                );
                ui.label(RichText::new("session score").size(12.5).color(theme::TEXT_DIM));

                ui.add_space(22.0);
                modality_bar(ui, "Position", result.position, adv, fb);
                ui.add_space(10.0);
                modality_bar(ui, "Audio", result.audio, adv, fb);

                ui.add_space(30.0);
                ui.horizontal(|ui| {
                    let row_w = 170.0 + 2.0 * 116.0 + 2.0 * ui.spacing().item_spacing.x;
                    ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
                    if ui
                        .add_sized(
                            [170.0, 46.0],
                            Button::new(
                                RichText::new("Play again")
                                    .strong()
                                    .color(Color32::from_rgb(0x0c, 0x12, 0x1f)),
                            )
                            .fill(theme::ACCENT),
                        )
                        .clicked()
                    {
                        again_clicked = true;
                    }
                    if ui.add_sized([116.0, 46.0], Button::new("Stats")).clicked() {
                        stats_clicked = true;
                    }
                    if ui.add_sized([116.0, 46.0], Button::new("Menu")).clicked() {
                        menu_clicked = true;
                    }
                });
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Space — play again · Esc — menu")
                        .size(12.0)
                        .color(theme::TEXT_DIM),
                );
            });
        });

        if again_clicked || again_key {
            self.start_session();
        } else if stats_clicked || stats_key {
            self.screen = Screen::Stats;
        } else if menu_clicked || menu_key {
            self.screen = Screen::Menu;
        }
    }

    fn draw_stats(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.screen = Screen::Menu;
            return;
        }
        let mut back_clicked = false;

        CentralPanel::default()
            .frame(Frame::NONE.fill(theme::BG).inner_margin(Margin::symmetric(36, 24)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("←  Menu").clicked() {
                        back_clicked = true;
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("Statistics").size(24.0).strong());
                });
                ui.add_space(12.0);

                if self.history.records.is_empty() {
                    ui.add_space(80.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No sessions yet — play one!")
                                .size(16.0)
                                .color(theme::TEXT_DIM),
                        );
                    });
                    return;
                }

                let best = self.history.best_level().unwrap_or(1);
                let today = self.history.sessions_today();
                let avg = self
                    .history
                    .avg_score_recent(7)
                    .map_or_else(|| "—".to_owned(), |v| format!("{v}%"));
                let total = self.history.records.len();

                let card_gap = ui.spacing().item_spacing.x;
                let card_w = (ui.available_width() - 3.0 * card_gap) / 4.0;
                ui.horizontal(|ui| {
                    for (title, value) in [
                        ("Best level", format!("D{best}B")),
                        ("Today", format!("{today}")),
                        ("7-day avg score", avg),
                        ("Total sessions", format!("{total}")),
                    ] {
                        Frame::NONE
                            .fill(theme::PANEL)
                            .corner_radius(CornerRadius::same(12))
                            .inner_margin(Margin::symmetric(16, 12))
                            .show(ui, |ui| {
                                ui.set_min_width(card_w - 32.0);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(value).size(24.0).strong());
                                    ui.label(
                                        RichText::new(title).size(12.0).color(theme::TEXT_DIM),
                                    );
                                });
                            });
                    }
                });

                ui.add_space(14.0);
                draw_chart(
                    ui,
                    &self.history.records,
                    self.config.threshold_advance,
                    self.config.threshold_fallback,
                );

                ui.add_space(16.0);
                ui.label(RichText::new("Recent sessions").size(16.0).strong());
                ui.add_space(4.0);
                ScrollArea::vertical().show(ui, |ui| {
                    Grid::new("recent-sessions")
                        .striped(true)
                        .num_columns(6)
                        .spacing([26.0, 8.0])
                        .show(ui, |ui| {
                            for header in ["When", "Level", "Position", "Audio", "Score", "Δ"] {
                                ui.label(
                                    RichText::new(header).size(12.0).color(theme::TEXT_DIM),
                                );
                            }
                            ui.end_row();
                            for r in self.history.records.iter().rev().take(20) {
                                ui.label(
                                    RichText::new(r.ts.format("%b %d · %H:%M").to_string())
                                        .size(13.0)
                                        .color(theme::TEXT_DIM),
                                );
                                let level = if r.manual {
                                    format!("D{}B ·M", r.n)
                                } else {
                                    format!("D{}B", r.n)
                                };
                                ui.label(RichText::new(level).size(13.5));
                                ui.label(RichText::new(format!("{}%", r.position_pct)).size(13.5));
                                ui.label(RichText::new(format!("{}%", r.audio_pct)).size(13.5));
                                ui.label(
                                    RichText::new(format!("{}%", r.score)).size(13.5).strong().color(
                                        theme::score_color(
                                            r.score,
                                            self.config.threshold_advance,
                                            self.config.threshold_fallback,
                                        ),
                                    ),
                                );
                                let (delta, color) = match r.outcome {
                                    Outcome::Advance => ("+1", theme::GREEN),
                                    Outcome::Fallback => ("−1", theme::RED),
                                    Outcome::Stay => ("—", theme::TEXT_DIM),
                                };
                                ui.label(RichText::new(delta).size(13.5).color(color));
                                ui.end_row();
                            }
                        });
                });
            });

        if back_clicked {
            self.screen = Screen::Menu;
        }
    }

    fn draw_settings(&mut self, ctx: &Context, ui: &mut egui::Ui) {
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.confirm_reset = false;
            self.save_config_if_dirty();
            self.screen = Screen::Menu;
            return;
        }
        let mut back_clicked = false;
        let mut dirty = false;

        CentralPanel::default()
            .frame(Frame::NONE.fill(theme::BG).inner_margin(Margin::symmetric(36, 24)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("←  Menu").clicked() {
                        back_clicked = true;
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("Settings").size(24.0).strong());
                });

                ScrollArea::vertical().show(ui, |ui| {
                    section(ui, "Game");
                    Grid::new("settings-game").num_columns(2).spacing([30.0, 12.0]).show(
                        ui,
                        |ui| {
                            ui.label("Level (n)");
                            let mut n = self.config.n;
                            if ui.add(DragValue::new(&mut n).range(1..=15)).changed() {
                                self.config.n = n;
                                self.config.fallback_streak = 0;
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label("Adaptive level");
                            dirty |= ui
                                .checkbox(&mut self.config.adaptive, "raise and lower n by score")
                                .changed();
                            ui.end_row();

                            ui.label("Trial duration");
                            let mut secs = self.config.trial_ms as f32 / 1000.0;
                            if ui
                                .add(Slider::new(&mut secs, 1.5..=5.0).step_by(0.1).suffix(" s"))
                                .changed()
                            {
                                self.config.trial_ms = (secs * 1000.0).round() as u32;
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label("Square visible for");
                            let mut stim = self.config.stim_ms as f32 / 1000.0;
                            if ui
                                .add(Slider::new(&mut stim, 0.2..=1.5).step_by(0.05).suffix(" s"))
                                .changed()
                            {
                                self.config.stim_ms = (stim * 1000.0).round() as u32;
                                dirty = true;
                            }
                            ui.end_row();

                            ui.label("Trials per session");
                            ui.horizontal(|ui| {
                                let mut base = self.config.base_trials;
                                if ui.add(Slider::new(&mut base, 10..=30)).changed() {
                                    self.config.base_trials = base;
                                    dirty = true;
                                }
                                ui.label(
                                    RichText::new(format!(
                                        "+ n²  =  {} at n = {}",
                                        self.config.total_trials(),
                                        self.config.n
                                    ))
                                    .size(12.5)
                                    .color(theme::TEXT_DIM),
                                );
                            });
                            ui.end_row();
                        },
                    );

                    section(ui, "Leveling");
                    Grid::new("settings-level").num_columns(2).spacing([30.0, 12.0]).show(
                        ui,
                        |ui| {
                            ui.label("Advance at");
                            dirty |= ui
                                .add(
                                    Slider::new(&mut self.config.threshold_advance, 60..=100)
                                        .suffix("%"),
                                )
                                .changed();
                            ui.end_row();

                            ui.label("Fall back below");
                            dirty |= ui
                                .add(
                                    Slider::new(&mut self.config.threshold_fallback, 20..=70)
                                        .suffix("%"),
                                )
                                .changed();
                            ui.end_row();

                            ui.label("Strikes to fall back");
                            dirty |= ui
                                .add(Slider::new(&mut self.config.fallback_sessions, 1..=5))
                                .changed();
                            ui.end_row();
                        },
                    );

                    section(ui, "Feedback & sound");
                    Grid::new("settings-sound").num_columns(2).spacing([30.0, 12.0]).show(
                        ui,
                        |ui| {
                            ui.label("Press feedback");
                            dirty |= ui
                                .checkbox(&mut self.config.feedback, "show right / wrong / missed")
                                .changed();
                            ui.end_row();

                            ui.label("Audio set");
                            ui.horizontal(|ui| {
                                let label = match self.config.audio_set {
                                    AudioSet::Letters => "Spoken letters",
                                    AudioSet::Tones => "Tones",
                                };
                                ComboBox::from_id_salt("audio-set")
                                    .selected_text(label)
                                    .show_ui(ui, |ui| {
                                        dirty |= ui
                                            .selectable_value(
                                                &mut self.config.audio_set,
                                                AudioSet::Letters,
                                                "Spoken letters",
                                            )
                                            .changed();
                                        dirty |= ui
                                            .selectable_value(
                                                &mut self.config.audio_set,
                                                AudioSet::Tones,
                                                "Tones",
                                            )
                                            .changed();
                                    });
                                if ui.button("Test").clicked() {
                                    self.audio.play_stimulus(self.config.audio_set, 2);
                                }
                            });
                            ui.end_row();

                            ui.label("Volume");
                            if ui.add(Slider::new(&mut self.config.volume, 0.0..=1.0)).changed() {
                                self.audio.volume = self.config.volume;
                                dirty = true;
                            }
                            ui.end_row();
                        },
                    );

                    section(ui, "Data");
                    let reset_label = if self.confirm_reset {
                        "Really erase all history and reset to Dual 2-Back?"
                    } else {
                        "Reset progress…"
                    };
                    if ui.button(RichText::new(reset_label).color(theme::RED)).clicked() {
                        if self.confirm_reset {
                            self.history.clear();
                            self.config.n = 2;
                            self.config.fallback_streak = 0;
                            self.config.save();
                            self.confirm_reset = false;
                        } else {
                            self.confirm_reset = true;
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!("stored in {}", crate::config::data_dir().display()))
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                    );
                    ui.add_space(20.0);
                });
            });

        // Keep the thresholds coherent.
        if self.config.threshold_fallback > self.config.threshold_advance {
            self.config.threshold_fallback = self.config.threshold_advance;
        }
        if dirty {
            self.config_dirty = true;
        }
        if back_clicked {
            self.confirm_reset = false;
            self.save_config_if_dirty();
            self.screen = Screen::Menu;
        }
    }
}

fn press_feedback(press: Option<Press>) -> Option<bool> {
    match press {
        Some(Press::Correct) => Some(true),
        Some(Press::Wrong) => Some(false),
        Some(Press::Unscored) | None => None,
    }
}

/// Draw one input chip (key badge + label) and report clicks.
fn draw_chip(
    ui: &egui::Ui,
    rect: Rect,
    id: &str,
    key_label: &str,
    title: &str,
    pressed: bool,
    flash: Option<Flash>,
) -> bool {
    let response = ui.interact(rect, Id::new(id), Sense::click());
    let painter = ui.painter();

    let (bg, stroke) = if let Some(f) = flash {
        let t = (f.remaining_ms / FLASH_MS).clamp(0.0, 1.0);
        let color = if f.good { theme::GREEN } else { theme::RED };
        (mix(theme::CELL, color, 0.42 * t), theme::stroke(1.5, mix(theme::CELL_STROKE, color, t)))
    } else if pressed {
        (
            mix(theme::CELL, theme::ACCENT, 0.16),
            theme::stroke(1.5, theme::ACCENT.gamma_multiply(0.7)),
        )
    } else if response.hovered() {
        (theme::CELL, theme::stroke(1.0, theme::ACCENT.gamma_multiply(0.5)))
    } else {
        (theme::CELL, theme::stroke(1.0, theme::CELL_STROKE))
    };

    painter.rect_filled(rect, CornerRadius::same(13), bg);
    painter.rect_stroke(rect, CornerRadius::same(13), stroke, StrokeKind::Inside);

    let badge = Rect::from_center_size(
        pos2(rect.left() + 31.0, rect.center().y),
        vec2(30.0, 30.0),
    );
    painter.rect_filled(badge, CornerRadius::same(7), theme::BG.gamma_multiply(0.85));
    painter.rect_stroke(
        badge,
        CornerRadius::same(7),
        theme::stroke(1.0, theme::CELL_STROKE),
        StrokeKind::Inside,
    );
    painter.text(
        badge.center(),
        Align2::CENTER_CENTER,
        key_label,
        FontId::monospace(14.0),
        theme::TEXT,
    );
    painter.text(
        pos2(badge.right() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(15.5),
        theme::TEXT,
    );
    response.clicked()
}

/// A labelled horizontal score bar for one modality on the results screen.
fn modality_bar(ui: &mut egui::Ui, label: &str, score: ModalityScore, adv: u32, fb: u32) {
    let width = 430.0_f32.min(ui.available_width() - 40.0);
    let (_, rect) = ui.allocate_space(vec2(width, 52.0));
    let painter = ui.painter();
    let pct = score.percent();
    let color = theme::score_color(pct, adv, fb);

    painter.text(
        pos2(rect.left(), rect.top() + 8.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.5),
        theme::TEXT_DIM,
    );
    painter.text(
        pos2(rect.right(), rect.top() + 8.0),
        Align2::RIGHT_CENTER,
        format!("{pct}%"),
        FontId::proportional(14.5),
        color,
    );

    let bar = Rect::from_min_max(
        pos2(rect.left(), rect.top() + 20.0),
        pos2(rect.right(), rect.top() + 30.0),
    );
    painter.rect_filled(bar, CornerRadius::same(5), theme::CELL);
    if pct > 0 {
        let fill = Rect::from_min_size(
            bar.min,
            vec2(bar.width() * (pct as f32 / 100.0), bar.height()),
        );
        painter.rect_filled(fill, CornerRadius::same(5), color);
    }

    painter.text(
        pos2(rect.left(), rect.bottom() - 6.0),
        Align2::LEFT_CENTER,
        format!(
            "{} hits · {} missed · {} false alarms · {} matches",
            score.hits, score.misses, score.false_alarms, score.matches
        ),
        FontId::proportional(11.5),
        theme::TEXT_DIM,
    );
}

/// Score bars plus an n-level line for the last 40 sessions.
fn draw_chart(ui: &mut egui::Ui, records: &[SessionRecord], adv: u32, fb: u32) {
    let count = records.len().min(40);
    let recs = &records[records.len() - count..];
    let width = ui.available_width();
    let (response, painter) = ui.allocate_painter(vec2(width, 230.0), Sense::hover());
    let outer = response.rect;
    painter.rect_filled(outer, CornerRadius::same(12), theme::PANEL);

    let plot = Rect::from_min_max(
        pos2(outer.left() + 46.0, outer.top() + 16.0),
        pos2(outer.right() - 18.0, outer.bottom() - 34.0),
    );
    let y_pct = |p: f32| plot.bottom() - p / 100.0 * plot.height();

    for pct in [0u32, 100] {
        let y = y_pct(pct as f32);
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            theme::stroke(1.0, theme::CELL_STROKE.gamma_multiply(0.7)),
        );
        painter.text(
            pos2(plot.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            format!("{pct}"),
            FontId::proportional(10.5),
            theme::TEXT_DIM,
        );
    }
    for (pct, color) in [(adv, theme::GREEN), (fb, theme::RED)] {
        let y = y_pct(pct as f32);
        painter.extend(Shape::dashed_line(
            &[pos2(plot.left(), y), pos2(plot.right(), y)],
            theme::stroke(1.0, color.gamma_multiply(0.45)),
            5.0,
            5.0,
        ));
        painter.text(
            pos2(plot.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            format!("{pct}"),
            FontId::proportional(10.5),
            color.gamma_multiply(0.8),
        );
    }

    let slot = plot.width() / count as f32;
    let bar_w = (slot * 0.6).clamp(2.0, 24.0);
    for (k, r) in recs.iter().enumerate() {
        let cx = plot.left() + slot * (k as f32 + 0.5);
        let color = match r.outcome {
            Outcome::Advance => theme::GREEN.gamma_multiply(0.8),
            Outcome::Fallback => theme::RED.gamma_multiply(0.75),
            Outcome::Stay => theme::SLATE,
        };
        let top = y_pct(r.score as f32).min(plot.bottom() - 1.5);
        painter.rect_filled(
            Rect::from_min_max(pos2(cx - bar_w / 2.0, top), pos2(cx + bar_w / 2.0, plot.bottom())),
            CornerRadius::same(2),
            color,
        );
    }

    let max_n = recs.iter().map(|r| r.n).max().unwrap_or(2).max(3) + 1;
    let y_n = |n: usize| plot.bottom() - n as f32 / max_n as f32 * plot.height();
    let points: Vec<Pos2> = recs
        .iter()
        .enumerate()
        .map(|(k, r)| pos2(plot.left() + slot * (k as f32 + 0.5), y_n(r.n)))
        .collect();
    if points.len() >= 2 {
        painter.add(Shape::line(points.clone(), theme::stroke(2.0, theme::ACCENT)));
    }
    for p in &points {
        painter.circle_filled(*p, 2.6, theme::ACCENT);
    }

    painter.text(
        pos2(plot.left(), outer.bottom() - 16.0),
        Align2::LEFT_CENTER,
        format!("bars — session score · line — n level · last {count} sessions"),
        FontId::proportional(11.0),
        theme::TEXT_DIM,
    );
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(16.0);
    ui.label(RichText::new(title.to_uppercase()).size(11.5).strong().color(theme::TEXT_DIM));
    ui.add_space(2.0);
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt_ms = (now - self.last_frame).as_secs_f32().min(0.2) * 1000.0;
        self.last_frame = now;
        let screen_at_start = self.screen;

        if self.screen == Screen::Game {
            self.update_game(&ctx, dt_ms);
        }

        match self.screen {
            Screen::Menu => self.draw_menu(&ctx, ui),
            Screen::Game => self.draw_game(ui),
            Screen::Results => self.draw_results(&ctx, ui),
            Screen::Stats => self.draw_stats(&ctx, ui),
            Screen::Settings => self.draw_settings(&ctx, ui),
        }

        // A screen switch decided this frame must still get painted.
        if self.screen != screen_at_start {
            ctx.request_repaint();
        }
    }

    fn on_exit(&mut self) {
        self.save_config_if_dirty();
    }
}
