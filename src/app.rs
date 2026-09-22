//! Terminal lifecycle and the fixed-tick game loop.
//!
//! The loop runs at a fixed 60 Hz because both rulesets are specified in frames:
//! NES's gravity, DAS and entry-delay tables are frame counts, and modern lock
//! delay is 500ms, which is 30 of these ticks. Rendering is driven by the same
//! clock; input is drained non-blockingly within each tick's budget.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::config::Config;
use crate::game::{Game, Input, Mode};
use crate::input::action::Action;
use crate::input::held::{HeldKeys, TimingMode};
use crate::input::keymap::Keymap;
use crate::ui::{board_view, hud, layout};

const TICK: Duration = Duration::from_nanos(16_666_667);

type Backend = CrosstermBackend<Stdout>;

pub struct App {
    game: Game,
    keymap: Keymap,
    held: HeldKeys,
    paused: bool,
    should_quit: bool,
    /// Edge-triggered actions collected since the last tick.
    pressed: Vec<Action>,
    config: Config,
    /// The level this run began on, which is what gets remembered — not the level
    /// play has since climbed to.
    start_level: u32,
}

impl App {
    pub fn new(mode: Mode, start_level: u32, timing: TimingMode) -> Self {
        Self::with_config(mode, start_level, timing, Config::default())
    }

    pub fn with_config(mode: Mode, start_level: u32, timing: TimingMode, config: Config) -> Self {
        Self {
            game: Game::with_settings(mode, start_level, config.modern_settings()),
            keymap: config.keymap(),
            held: HeldKeys::new(timing),
            paused: false,
            should_quit: false,
            pressed: Vec::new(),
            config,
            start_level,
        }
    }

    /// Remember how this session was set up, so a bare launch resumes it.
    fn remember_session(&mut self) {
        let mode = self.game.mode();
        self.config.mode = mode;
        self.config.set_start_level(mode, self.start_level);
        // Failing to write settings must never be worth interrupting play over.
        let _ = self.config.save();
    }

    fn handle_event(&mut self, event: Event, now: Instant) {
        let Event::Key(key) = event else { return };
        let Some(action) = self.keymap.action_for(&key) else {
            return;
        };

        match key.kind {
            KeyEventKind::Press | KeyEventKind::Repeat => {
                if action.is_held() {
                    self.held.press(action, now);
                } else if key.kind == KeyEventKind::Press {
                    // Rotation and menu actions fire once per press; autorepeat must
                    // not spin the piece.
                    self.pressed.push(action);
                }
            }
            KeyEventKind::Release => {
                if action.is_held() {
                    self.held.release(action);
                }
            }
        }
    }

    fn frame_input(&mut self, now: Instant) -> Input {
        Input {
            left: self.held.is_held(Action::MoveLeft, now),
            right: self.held.is_held(Action::MoveRight, now),
            soft_drop: self.held.is_held(Action::SoftDrop, now),
            hard_drop: self.pressed.contains(&Action::HardDrop),
            rotate_cw: self.pressed.contains(&Action::RotateCw),
            rotate_ccw: self.pressed.contains(&Action::RotateCcw),
            hold: self.pressed.contains(&Action::Hold),
        }
    }

    fn tick(&mut self, now: Instant) {
        if self.pressed.contains(&Action::Quit) {
            self.should_quit = true;
        }
        if self.pressed.contains(&Action::Pause) {
            self.paused = !self.paused;
        }

        if !self.paused {
            let input = self.frame_input(now);
            self.game.tick(input);
        }

        self.pressed.clear();
        self.held.expire(now);
    }

    fn draw(&self, frame: &mut Frame) {
        let plan = layout::compute(frame.area(), self.game.preview().len());

        if plan.tier == layout::Tier::TooSmall {
            let message = format!(
                "Terminal too small\n\nNeed at least {}x{}",
                layout::MIN_WIDTH,
                layout::MIN_HEIGHT
            );
            frame.render_widget(
                Paragraph::new(message)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: true }),
                frame.area(),
            );
            return;
        }

        let title = if self.paused {
            " PAUSED ".to_string()
        } else {
            format!(" {} ", self.game.mode().label())
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::DarkGray));
        let interior = block.inner(plan.board);
        frame.render_widget(block, plan.board);

        board_view::render(frame.buffer_mut(), interior, &self.game);

        if let Some(area) = plan.stats {
            hud::render_stats(frame, area, &self.game, self.held.mode().label());
        }
        if let Some(area) = plan.next {
            hud::render_next(frame, area, &self.game);
        }
        if let Some(area) = plan.piece_counts {
            hud::render_side_panel(frame, area, &self.game);
        }

        if self.game.is_over() {
            self.draw_overlay(frame, interior, "GAME OVER", "q to quit");
        } else if self.paused {
            self.draw_overlay(frame, interior, "PAUSED", "p to resume");
        }
    }

    fn draw_overlay(&self, frame: &mut Frame, area: Rect, title: &str, hint: &str) {
        let height = 4;
        let width = area.width.min(18);
        let rect = Rect::new(
            area.x + (area.width.saturating_sub(width)) / 2,
            area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        );
        frame.render_widget(
            Paragraph::new(format!("{title}\n{hint}"))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::White))
                .block(Block::default().borders(Borders::ALL)),
            rect,
        );
    }
}

pub fn run(mode: Option<Mode>, start_level: Option<u32>) -> io::Result<()> {
    let config = Config::load();
    let mode = mode.unwrap_or(config.mode);
    let start_level = start_level.unwrap_or_else(|| config.start_level(mode));
    let precise = supports_keyboard_enhancement().unwrap_or(false);
    let mut terminal = setup(precise)?;

    let timing = if precise {
        TimingMode::Precise
    } else {
        TimingMode::Inferred
    };
    let mut app = App::with_config(mode, start_level, timing, config);

    let result = event_loop(&mut terminal, &mut app);
    app.remember_session();
    restore(precise)?;
    result
}

fn event_loop(terminal: &mut Terminal<Backend>, app: &mut App) -> io::Result<()> {
    let mut next_tick = Instant::now();

    while !app.should_quit {
        let now = Instant::now();

        // Drain whatever input is waiting without overrunning the tick budget.
        while event::poll(Duration::ZERO)? {
            let event = event::read()?;
            app.handle_event(event, now);
        }

        app.tick(now);
        terminal.draw(|frame| app.draw(frame))?;

        next_tick += TICK;
        let now = Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else {
            // Fell behind (a slow redraw, or the process was suspended): resync
            // rather than trying to catch up with a burst of ticks.
            next_tick = now;
        }
    }
    Ok(())
}

fn setup(precise: bool) -> io::Result<Terminal<Backend>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)?;
    if precise {
        // Real press/release events, which is what makes DAS frame-accurate.
        stdout.execute(PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::REPORT_EVENT_TYPES,
        ))?;
    }

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore(precise);
        previous_hook(info);
    }));

    Terminal::new(CrosstermBackend::new(io::stdout()))
}

fn restore(precise: bool) -> io::Result<()> {
    let mut stdout = io::stdout();
    if precise {
        let _ = stdout.execute(PopKeyboardEnhancementFlags);
    }
    execute!(stdout, LeaveAlternateScreen, crossterm::cursor::Show)?;
    disable_raw_mode()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventState, KeyModifiers};

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    fn release(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn quit_action_stops_the_loop() {
        let mut app = App::new(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();
        app.handle_event(press(KeyCode::Char('q')), now);
        app.tick(now);
        assert!(app.should_quit);
    }

    #[test]
    fn pause_toggles_and_freezes_the_game() {
        let mut app = App::new(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();
        let y_before = app.game.current().unwrap().y;

        app.handle_event(press(KeyCode::Char('p')), now);
        app.tick(now);
        assert!(app.paused);

        for _ in 0..200 {
            app.tick(now);
        }
        assert_eq!(
            app.game.current().unwrap().y,
            y_before,
            "a paused game must not fall"
        );

        app.handle_event(press(KeyCode::Char('p')), now);
        app.tick(now);
        assert!(!app.paused);
    }

    #[test]
    fn held_directions_reach_the_engine_and_stop_on_release() {
        let mut app = App::new(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();
        let x_before = app.game.current().unwrap().x;

        app.handle_event(press(KeyCode::Left), now);
        app.tick(now);
        assert_eq!(app.game.current().unwrap().x, x_before - 1);

        app.handle_event(release(KeyCode::Left), now);
        for _ in 0..40 {
            app.tick(now);
        }
        assert_eq!(
            app.game.current().unwrap().x,
            x_before - 1,
            "released key must not keep shifting"
        );
    }

    /// Autorepeat on a rotate key must not spin the piece: rotation is per press.
    #[test]
    fn autorepeat_does_not_retrigger_rotation() {
        let mut app = App::new(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();

        let repeat = Event::Key(KeyEvent {
            code: KeyCode::Char('x'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Repeat,
            state: KeyEventState::NONE,
        });
        app.handle_event(repeat, now);
        assert!(
            app.pressed.is_empty(),
            "repeat events must not queue a rotation"
        );
    }

    fn render_to_string(app: &mut App, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;

        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();

        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                rendered.push_str(buffer[(x, y)].symbol());
            }
            rendered.push('\n');
        }
        rendered
    }

    /// Renders real frames through ratatui's test backend and dumps them, so the
    /// layout can be inspected rather than assumed.
    #[test]
    fn renders_a_frame_in_each_mode() {
        for (mode, level) in [(Mode::Nes, 0), (Mode::Modern, 1)] {
            let mut app = App::new(mode, level, TimingMode::Precise);
            // Let a piece fall so the field is not empty.
            for _ in 0..120 {
                app.tick(Instant::now());
            }

            let rendered = render_to_string(&mut app, 80, 26);
            println!("=== {} ===\n{rendered}", mode.label());

            assert!(rendered.contains(mode.label()));
            assert!(rendered.contains("SCORE"));
            assert!(rendered.contains("NEXT"));

            // The side panel follows the ruleset.
            match mode {
                Mode::Nes => assert!(rendered.contains("STATS")),
                Mode::Modern => assert!(rendered.contains("HOLD")),
            }
        }
    }

    #[test]
    fn renders_a_resize_prompt_when_the_terminal_is_tiny() {
        use ratatui::backend::TestBackend;

        let app = App::new(Mode::Nes, 0, TimingMode::Precise);
        let mut terminal = Terminal::new(TestBackend::new(20, 10)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();

        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                rendered.push_str(buffer[(x, y)].symbol());
            }
        }
        assert!(rendered.contains("too small"), "got: {rendered}");
    }

    #[test]
    fn a_press_queues_exactly_one_rotation() {
        let mut app = App::new(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();
        app.handle_event(press(KeyCode::Char('x')), now);
        assert_eq!(app.pressed, vec![Action::RotateCw]);
        app.tick(now);
        assert!(app.pressed.is_empty(), "pressed actions clear each tick");
    }
}
