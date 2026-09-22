//! Terminal lifecycle, the app state machine, and the fixed-tick loop.
//!
//! The loop runs at a fixed 60 Hz because both rulesets are specified in frames:
//! NES's gravity, DAS and entry-delay tables are frame counts, and modern lock
//! delay is 500ms, which is 30 of these ticks. Rendering is driven by the same
//! clock; input is drained non-blockingly within each tick's budget.
//!
//! Menus run through the very same loop rather than a blocking read of their own,
//! so nothing has to be restructured when animated backgrounds arrive and the
//! title screen has to keep drawing while it waits for a key.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
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
use crate::menu::{
    menu_input, GameOverItem, GameOverMenu, OptionsMenu, OptionsOutcome, PauseItem, PauseMenu,
    ScoresView, TitleItem, TitleMenu,
};
use crate::scores::{today, Entry, Scores};
use crate::ui::{board_view, hud, layout, menu as menu_ui};

const TICK: Duration = Duration::from_nanos(16_666_667);

type Backend = CrosstermBackend<Stdout>;

/// Which screen is in front of the player. Everything except `Playing` leaves the
/// engine untouched, so a paused or options-bound game resumes exactly where it
/// stopped.
#[derive(Debug)]
pub enum AppState {
    Title(TitleMenu),
    Options(OptionsMenu),
    HighScores(ScoresView),
    Playing,
    Paused(PauseMenu),
    GameOver(GameOverMenu),
}

/// Where leaving the options screen should return to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionsOrigin {
    Title,
    Pause,
}

pub struct App {
    state: AppState,
    /// Absent before the first run and after quitting one back to the title.
    game: Option<Game>,
    config: Config,
    scores: Scores,
    keymap: Keymap,
    held: HeldKeys,
    /// Edge-triggered actions collected since the last tick.
    pressed: Vec<Action>,
    should_quit: bool,
    /// The level the current run began on, which is what gets remembered — not the
    /// level play has since climbed to.
    start_level: u32,
    options_origin: OptionsOrigin,
    /// Whether settings and scores are written to disk. Off in tests, which must
    /// not touch the player's real files.
    persist: bool,
}

impl App {
    /// Start at the title screen with whatever settings are on disk.
    pub fn new(timing: TimingMode, config: Config, scores: Scores) -> Self {
        Self {
            state: AppState::Title(TitleMenu::default()),
            game: None,
            keymap: config.keymap(),
            held: HeldKeys::new(timing),
            pressed: Vec::new(),
            should_quit: false,
            start_level: config.start_level(config.mode),
            options_origin: OptionsOrigin::Title,
            persist: true,
            scores,
            config,
        }
    }

    #[cfg(test)]
    fn headless(mode: Mode, start_level: u32, timing: TimingMode) -> Self {
        let mut config = Config {
            mode,
            ..Default::default()
        };
        config.set_start_level(mode, start_level);
        let mut app = Self::new(timing, config, Scores::default());
        app.persist = false;
        app.start_run();
        app.state = AppState::Playing;
        app
    }

    /// Build a fresh game from the current settings. The caller moves to
    /// `Playing`: menu handlers own their own transition.
    fn start_run(&mut self) {
        let mode = self.config.mode;
        self.start_level = self.config.start_level(mode);
        self.game = Some(Game::with_settings(
            mode,
            self.start_level,
            self.config.modern_settings(),
        ));
        // A key still down from the menu must not shift the first piece.
        self.held.clear();
        self.pressed.clear();
    }

    /// Remember how this session was set up, so a bare launch resumes it.
    fn remember_session(&mut self) {
        self.save_config();
    }

    /// Persist a settings change and pick up anything that can take effect now.
    fn apply_config(&mut self) {
        // Rebinding applies immediately; the modern timing settings are read when
        // a run starts, so changing them mid-run affects the next one.
        self.keymap = self.config.keymap();
        self.save_config();
    }

    /// Failing to write settings must never be worth interrupting play over, so
    /// the error is deliberately dropped. `persist` is what keeps the test suite
    /// out of the real config and score files.
    fn save_config(&self) {
        if self.persist {
            let _ = self.config.save();
        }
    }

    fn save_scores(&self) {
        if self.persist {
            let _ = self.scores.save();
        }
    }

    // -- input ------------------------------------------------------------

    fn handle_event(&mut self, event: Event, now: Instant) {
        let Event::Key(key) = event else { return };

        // Raw mode swallows the terminal's own interrupt, so Ctrl-C is handled
        // here instead: from any screen it leaves, rather than being read as the
        // hold key that plain `c` is bound to.
        if key.kind != KeyEventKind::Release
            && key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.should_quit = true;
            return;
        }

        if matches!(self.state, AppState::Playing) {
            self.handle_play_key(key, now);
        } else if key.kind == KeyEventKind::Press {
            self.handle_menu_key(key);
        }
    }

    fn handle_play_key(&mut self, key: KeyEvent, now: Instant) {
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

    /// Menu screens are driven by their own fixed keys rather than the rebindable
    /// gameplay map — see the note in `crate::menu`.
    fn handle_menu_key(&mut self, key: KeyEvent) {
        // The state is moved out so each handler can take `&mut self` for the
        // config, scores and game it needs; anything that does not transition is
        // put back untouched.
        let mut state = std::mem::replace(&mut self.state, AppState::Playing);
        let next = match &mut state {
            AppState::Title(menu) => self.title_key(menu, key),
            AppState::Options(menu) => self.options_key(menu, key),
            AppState::HighScores(view) => self.scores_key(view, key),
            AppState::Paused(menu) => self.pause_key(menu, key),
            AppState::GameOver(menu) => self.game_over_key(menu, key),
            AppState::Playing => None,
        };
        self.state = next.unwrap_or(state);
    }

    fn title_key(&mut self, menu: &mut TitleMenu, key: KeyEvent) -> Option<AppState> {
        match menu.navigate(menu_input(&key)?)? {
            TitleItem::Play => {
                self.start_run();
                Some(AppState::Playing)
            }
            TitleItem::Options => {
                self.options_origin = OptionsOrigin::Title;
                Some(AppState::Options(OptionsMenu::default()))
            }
            TitleItem::HighScores => Some(AppState::HighScores(ScoresView::new(self.config.mode))),
            TitleItem::Quit => {
                self.should_quit = true;
                None
            }
        }
    }

    fn options_key(&mut self, menu: &mut OptionsMenu, key: KeyEvent) -> Option<AppState> {
        let outcome = if menu.rebinding.is_some() {
            // Every key is fair game while capturing a binding, so menu navigation
            // is deliberately bypassed here.
            menu.capture(&key, &mut self.config)
        } else {
            menu.navigate(menu_input(&key)?, &mut self.config)
        };

        match outcome {
            OptionsOutcome::Changed => {
                self.apply_config();
                None
            }
            OptionsOutcome::Stay => None,
            OptionsOutcome::Back => Some(match self.options_origin {
                OptionsOrigin::Title => AppState::Title(TitleMenu::default()),
                OptionsOrigin::Pause => AppState::Paused(PauseMenu::default()),
            }),
        }
    }

    fn scores_key(&mut self, view: &mut ScoresView, key: KeyEvent) -> Option<AppState> {
        view.navigate(menu_input(&key)?)
            .then(|| AppState::Title(TitleMenu::default()))
    }

    fn pause_key(&mut self, menu: &mut PauseMenu, key: KeyEvent) -> Option<AppState> {
        // The pause key itself resumes, as well as the menu's own Back.
        if self.keymap.action_for(&key) == Some(Action::Pause) {
            return Some(AppState::Playing);
        }

        match menu.navigate(menu_input(&key)?)? {
            PauseItem::Resume => {
                // Holds from before the pause are stale by now.
                self.held.clear();
                Some(AppState::Playing)
            }
            PauseItem::Options => {
                self.options_origin = OptionsOrigin::Pause;
                Some(AppState::Options(OptionsMenu::default()))
            }
            PauseItem::QuitToTitle => {
                // An abandoned run is not a result, so it is not scored.
                self.game = None;
                Some(AppState::Title(TitleMenu::default()))
            }
        }
    }

    fn game_over_key(&mut self, menu: &mut GameOverMenu, key: KeyEvent) -> Option<AppState> {
        if menu.entering {
            match key.code {
                KeyCode::Enter => {
                    if let Some(name) = menu.submit() {
                        self.config.player_name = name.clone();
                        self.save_config();
                        menu.rank = self.record_score(name);
                    }
                }
                KeyCode::Backspace => {
                    menu.backspace();
                }
                // Escape declines the entry, leaving the run unrecorded.
                KeyCode::Esc => menu.entering = false,
                KeyCode::Char(c) => {
                    menu.type_char(c);
                }
                _ => {}
            }
            return None;
        }

        match menu.navigate(menu_input(&key)?)? {
            GameOverItem::Retry => {
                self.start_run();
                Some(AppState::Playing)
            }
            GameOverItem::Title => {
                self.game = None;
                Some(AppState::Title(TitleMenu::default()))
            }
        }
    }

    fn record_score(&mut self, name: String) -> Option<usize> {
        let game = self.game.as_ref()?;
        let entry = Entry {
            name,
            score: game.score(),
            lines: game.lines(),
            level: game.level(),
            date: today(),
        };
        let rank = self.scores.insert(game.mode(), entry);
        self.save_scores();
        rank
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
        if matches!(self.state, AppState::Playing) {
            if self.pressed.contains(&Action::Quit) {
                // Leaving a run mid-play abandons it rather than scoring it.
                self.game = None;
                self.state = AppState::Title(TitleMenu::default());
            } else if self.pressed.contains(&Action::Pause) {
                self.state = AppState::Paused(PauseMenu::default());
            } else {
                let input = self.frame_input(now);
                if let Some(game) = &mut self.game {
                    game.tick(input);
                }
                if self.game.as_ref().is_some_and(Game::is_over) {
                    self.enter_game_over();
                }
            }
        }

        self.pressed.clear();
        self.held.expire(now);
    }

    fn enter_game_over(&mut self) {
        let qualifies = self
            .game
            .as_ref()
            .is_some_and(|game| self.scores.qualifies(game.mode(), game.score()));
        self.held.clear();
        self.state = AppState::GameOver(GameOverMenu::new(qualifies, &self.config.player_name));
    }

    // -- rendering --------------------------------------------------------

    fn draw(&self, frame: &mut Frame) {
        let area = frame.area();

        // The board stays on screen behind the pause, game-over and (when a run is
        // in progress) options screens, so the stack the player is mid-way through
        // never disappears under a menu.
        let board_area = match (&self.state, &self.game) {
            (AppState::Title(_) | AppState::HighScores(_), _) | (_, None) => None,
            (_, Some(game)) => self.draw_play(frame, game),
        };
        let overlay_area = board_area.unwrap_or(area);

        match &self.state {
            AppState::Playing => {}
            AppState::Title(menu) => menu_ui::render_title(frame, area, menu, self.config.mode),
            AppState::Options(menu) => menu_ui::render_options(frame, area, menu, &self.config),
            AppState::HighScores(view) => menu_ui::render_scores(frame, area, view, &self.scores),
            AppState::Paused(menu) => menu_ui::render_pause(frame, overlay_area, menu),
            AppState::GameOver(menu) => {
                let (score, lines, level) = match &self.game {
                    Some(game) => (game.score(), game.lines(), game.level()),
                    None => (0, 0, 0),
                };
                menu_ui::render_game_over(frame, overlay_area, menu, score, lines, level);
            }
        }
    }

    /// Draws the playfield and its panels, returning the board's interior so an
    /// overlay can be centred on it rather than on the whole terminal.
    fn draw_play(&self, frame: &mut Frame, game: &Game) -> Option<Rect> {
        let plan = layout::compute(frame.area(), game.preview().len());

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
            return None;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", game.mode().label()))
            .border_style(Style::default().fg(Color::DarkGray));
        let interior = block.inner(plan.board);
        frame.render_widget(block, plan.board);

        board_view::render(frame.buffer_mut(), interior, game);

        if let Some(area) = plan.stats {
            hud::render_stats(frame, area, game, self.held.mode().label());
        }
        if let Some(area) = plan.next {
            hud::render_next(frame, area, game);
        }
        if let Some(area) = plan.piece_counts {
            hud::render_side_panel(frame, area, game);
        }

        Some(interior)
    }
}

pub fn run(mode: Option<Mode>, start_level: Option<u32>) -> io::Result<()> {
    let config = Config::load();
    let precise = supports_keyboard_enhancement().unwrap_or(false);
    let timing = if precise {
        TimingMode::Precise
    } else {
        TimingMode::Inferred
    };

    let mut config = config;
    if let Some(mode) = mode {
        config.mode = mode;
    }
    if let Some(level) = start_level {
        config.set_start_level(config.mode, level);
    }

    // A mode on the command line means "play this now"; a bare launch opens the
    // title screen.
    let mut app = App::new(timing, config, Scores::load());
    if mode.is_some() {
        app.start_run();
        app.state = AppState::Playing;
    }

    let mut terminal = setup(precise)?;
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
    use crate::menu::{MenuInput, OptionRow};
    use crossterm::event::KeyEventState;

    fn key_event(code: KeyCode, kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind,
            state: KeyEventState::NONE,
        })
    }

    fn press(code: KeyCode) -> Event {
        key_event(code, KeyEventKind::Press)
    }

    fn release(code: KeyCode) -> Event {
        key_event(code, KeyEventKind::Release)
    }

    /// Feed a key and run the tick it would land on.
    fn send(app: &mut App, code: KeyCode) {
        let now = Instant::now();
        app.handle_event(press(code), now);
        app.tick(now);
    }

    fn title_app() -> App {
        let mut app = App::new(TimingMode::Precise, Config::default(), Scores::default());
        app.persist = false;
        app
    }

    #[test]
    fn a_bare_launch_opens_the_title_screen_with_no_game_running() {
        let app = title_app();
        assert!(matches!(app.state, AppState::Title(_)));
        assert!(app.game.is_none());
    }

    #[test]
    fn play_starts_a_run_in_the_configured_mode() {
        let mut app = title_app();
        app.config.mode = Mode::Modern;
        app.config.set_start_level(Mode::Modern, 4);

        send(&mut app, KeyCode::Enter);
        assert!(matches!(app.state, AppState::Playing));
        let game = app.game.as_ref().expect("a run should be in progress");
        assert_eq!(game.mode(), Mode::Modern);
        assert_eq!(game.level(), 4);
    }

    #[test]
    fn quitting_from_the_title_screen_ends_the_program() {
        let mut app = title_app();
        send(&mut app, KeyCode::Esc);
        assert!(app.should_quit);
    }

    #[test]
    fn the_title_menu_reaches_options_and_high_scores() {
        let mut app = title_app();
        send(&mut app, KeyCode::Down);
        send(&mut app, KeyCode::Enter);
        assert!(matches!(app.state, AppState::Options(_)));

        send(&mut app, KeyCode::Esc);
        assert!(matches!(app.state, AppState::Title(_)));

        send(&mut app, KeyCode::Down);
        send(&mut app, KeyCode::Down);
        send(&mut app, KeyCode::Enter);
        assert!(matches!(app.state, AppState::HighScores(_)));
    }

    #[test]
    fn pausing_opens_the_pause_menu_and_freezes_the_game() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        let y_before = app.game.as_ref().unwrap().current().unwrap().y;

        send(&mut app, KeyCode::Char('p'));
        assert!(matches!(app.state, AppState::Paused(_)));

        for _ in 0..200 {
            app.tick(Instant::now());
        }
        assert_eq!(
            app.game.as_ref().unwrap().current().unwrap().y,
            y_before,
            "a paused game must not fall"
        );

        // The pause key resumes as well as opening the menu.
        send(&mut app, KeyCode::Char('p'));
        assert!(matches!(app.state, AppState::Playing));
    }

    #[test]
    fn the_pause_menu_can_quit_to_the_title_and_abandons_the_run() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        send(&mut app, KeyCode::Char('p'));
        // Resume, Options, Quit to title.
        send(&mut app, KeyCode::Down);
        send(&mut app, KeyCode::Down);
        send(&mut app, KeyCode::Enter);

        assert!(matches!(app.state, AppState::Title(_)));
        assert!(app.game.is_none(), "the abandoned run is dropped");
    }

    /// Options opened from the pause menu must return there, not to the title.
    #[test]
    fn options_return_to_wherever_they_were_opened_from() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        send(&mut app, KeyCode::Char('p'));
        send(&mut app, KeyCode::Down);
        send(&mut app, KeyCode::Enter);
        assert!(matches!(app.state, AppState::Options(_)));

        send(&mut app, KeyCode::Esc);
        assert!(matches!(app.state, AppState::Paused(_)));
    }

    #[test]
    fn quitting_during_play_returns_to_the_title_rather_than_exiting() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        send(&mut app, KeyCode::Char('q'));
        assert!(!app.should_quit, "one q leaves the run, not the program");
        assert!(matches!(app.state, AppState::Title(_)));

        send(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit, "a second q leaves the program");
    }

    /// Raw mode swallows the terminal's interrupt, so the app has to honour it
    /// itself — and it must not be read as the `c` that holds a piece.
    #[test]
    fn ctrl_c_leaves_from_any_screen() {
        for mut app in [
            title_app(),
            App::headless(Mode::Modern, 1, TimingMode::Precise),
        ] {
            let now = Instant::now();
            app.handle_event(
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                now,
            );
            app.tick(now);
            assert!(app.should_quit);
        }
    }

    #[test]
    fn held_directions_reach_the_engine_and_stop_on_release() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();
        let x_before = app.game.as_ref().unwrap().current().unwrap().x;

        app.handle_event(press(KeyCode::Left), now);
        app.tick(now);
        assert_eq!(
            app.game.as_ref().unwrap().current().unwrap().x,
            x_before - 1
        );

        app.handle_event(release(KeyCode::Left), now);
        for _ in 0..40 {
            app.tick(now);
        }
        assert_eq!(
            app.game.as_ref().unwrap().current().unwrap().x,
            x_before - 1,
            "released key must not keep shifting"
        );
    }

    /// Autorepeat on a rotate key must not spin the piece: rotation is per press.
    #[test]
    fn autorepeat_does_not_retrigger_rotation() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        app.handle_event(
            key_event(KeyCode::Char('x'), KeyEventKind::Repeat),
            Instant::now(),
        );
        assert!(
            app.pressed.is_empty(),
            "repeat events must not queue a rotation"
        );
    }

    #[test]
    fn a_press_queues_exactly_one_rotation() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();
        app.handle_event(press(KeyCode::Char('x')), now);
        assert_eq!(app.pressed, vec![Action::RotateCw]);
        app.tick(now);
        assert!(app.pressed.is_empty(), "pressed actions clear each tick");
    }

    /// Menu keys are not the gameplay keymap, so a rebind cannot lock the player
    /// out of the screen that would undo it.
    #[test]
    fn menu_navigation_ignores_the_gameplay_bindings() {
        let mut app = title_app();
        app.config.set_binding(Action::MoveLeft, &[KeyCode::Down]);
        app.apply_config();

        send(&mut app, KeyCode::Down);
        let AppState::Title(menu) = &app.state else {
            panic!("still on the title screen");
        };
        assert_eq!(menu.selected, 1, "Down still moves the menu cursor");
    }

    #[test]
    fn a_rebind_made_in_the_options_screen_takes_effect_immediately() {
        let mut app = title_app();
        app.options_origin = OptionsOrigin::Title;
        let rows = OptionsMenu::rows(&app.config);
        let selected = rows
            .iter()
            .position(|row| *row == OptionRow::Bind(Action::RotateCw))
            .unwrap();
        app.state = AppState::Options(OptionsMenu {
            selected,
            ..Default::default()
        });

        send(&mut app, KeyCode::Enter); // start rebinding
        send(&mut app, KeyCode::Char('n')); // capture the new key

        assert_eq!(
            app.keymap.action_for(&KeyEvent {
                code: KeyCode::Char('n'),
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            }),
            Some(Action::RotateCw)
        );
    }

    #[test]
    fn topping_out_moves_to_the_game_over_screen() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        fill_board_to_the_top(&mut app);

        for _ in 0..2000 {
            app.tick(Instant::now());
            if matches!(app.state, AppState::GameOver(_)) {
                break;
            }
        }
        assert!(
            matches!(app.state, AppState::GameOver(_)),
            "a buried board should top out"
        );
        assert!(app.game.is_some(), "the final board stays on screen");
    }

    /// Everything from the last row down is filled, leaving no room to spawn.
    fn fill_board_to_the_top(app: &mut App) {
        let game = app.game.as_mut().unwrap();
        let board = game.board_mut();
        for y in 0..board.height() {
            for x in 0..board.width() {
                if x != 0 {
                    board.set(x as i32, y as i32, Some(crate::engine::piece::PieceKind::O));
                }
            }
        }
    }

    #[test]
    fn a_qualifying_score_is_recorded_under_the_typed_name() {
        let mut app = App::headless(Mode::Modern, 1, TimingMode::Precise);
        // Hard drops score, so the run is worth recording.
        for _ in 0..3 {
            send(&mut app, KeyCode::Char(' '));
        }
        let score = app.game.as_ref().unwrap().score();
        assert!(score > 0, "the run should have scored something");

        app.state = AppState::GameOver(GameOverMenu::new(true, ""));
        for c in "ada".chars() {
            send(&mut app, KeyCode::Char(c));
        }
        send(&mut app, KeyCode::Enter);

        let table = app.scores.table(Mode::Modern);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].name, "ada");
        assert_eq!(table[0].score, score);
        assert_eq!(app.config.player_name, "ada", "the name is remembered");

        let AppState::GameOver(menu) = &app.state else {
            panic!("still on the game over screen")
        };
        assert!(!menu.entering, "name entry is finished");
        assert_eq!(menu.rank, Some(0));
    }

    /// A run that did not place is not written to the table at all.
    #[test]
    fn a_run_that_did_not_place_records_nothing() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        app.state = AppState::GameOver(GameOverMenu::new(false, "player"));

        // The first choice is Retry, so Back is what leaves without recording.
        send(&mut app, KeyCode::Esc);
        assert!(matches!(app.state, AppState::Title(_)));
        assert!(app.scores.table(Mode::Nes).is_empty());
    }

    #[test]
    fn retry_starts_a_fresh_run_in_the_same_mode() {
        let mut app = App::headless(Mode::Modern, 3, TimingMode::Precise);
        app.state = AppState::GameOver(GameOverMenu::new(false, "player"));

        send(&mut app, KeyCode::Enter); // Retry is the first choice
        assert!(matches!(app.state, AppState::Playing));
        let game = app.game.as_ref().unwrap();
        assert_eq!(game.mode(), Mode::Modern);
        assert_eq!(game.level(), 3);
        assert!(!game.is_over());
    }

    #[test]
    fn the_high_score_view_switches_tables_and_returns_to_the_title() {
        let mut app = title_app();
        app.state = AppState::HighScores(ScoresView::new(Mode::Nes));

        send(&mut app, KeyCode::Right);
        let AppState::HighScores(view) = &app.state else {
            panic!("still viewing scores")
        };
        assert_eq!(view.mode, Mode::Modern);

        send(&mut app, KeyCode::Esc);
        assert!(matches!(app.state, AppState::Title(_)));
    }

    /// The menu module owns navigation; this only pins that the app hands it the
    /// same inputs the player produces.
    #[test]
    fn enter_and_space_both_confirm() {
        for code in [KeyCode::Enter, KeyCode::Char(' ')] {
            let key = KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            };
            assert_eq!(menu_input(&key), Some(MenuInput::Confirm));
        }
    }

    fn render_to_string(app: &App, width: u16, height: u16) -> String {
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
            let mut app = App::headless(mode, level, TimingMode::Precise);
            // Let a piece fall so the field is not empty.
            for _ in 0..120 {
                app.tick(Instant::now());
            }

            let rendered = render_to_string(&app, 80, 26);
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
        let app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        let rendered = render_to_string(&app, 20, 10);
        assert!(rendered.contains("too small"), "got: {rendered}");
    }

    /// The board is still there behind the pause menu: a stack mid-game must not
    /// vanish under an overlay.
    #[test]
    fn the_pause_menu_is_drawn_over_the_board() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        send(&mut app, KeyCode::Char('p'));

        let rendered = render_to_string(&app, 80, 26);
        println!("{rendered}");
        assert!(rendered.contains("PAUSED"));
        assert!(rendered.contains("SCORE"), "the HUD is still drawn");
    }

    #[test]
    fn the_title_screen_draws_without_a_game() {
        let app = title_app();
        let rendered = render_to_string(&app, 80, 26);
        println!("{rendered}");
        assert!(rendered.contains("Play"));
        assert!(!rendered.contains("SCORE"), "no HUD before a run starts");
    }
}
