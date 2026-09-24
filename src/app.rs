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
    self, DisableFocusChange, EnableFocusChange, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Margin, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::background::scenes::SceneChoice;
use crate::background::{Background, BackgroundKind, Canvas, PerformanceSignal, PlacementHistory};
use crate::config::Config;
use crate::game::{Game, Input, Mode};
use crate::input::action::Action;
use crate::input::held::{HeldKeys, TimingMode};
use crate::input::keymap::{Keymap, MenuKeymap};
use crate::menu::{
    GameOverItem, GameOverMenu, MenuInput, OptionsMenu, OptionsOutcome, PauseItem, PauseMenu,
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
    menu_keys: MenuKeymap,
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
    background: Box<dyn Background>,
    /// What the background was built from, so it is rebuilt only when the setting
    /// actually changes rather than every frame.
    background_source: (BackgroundKind, SceneChoice),
    /// What this run's placements have done, which reactive backgrounds read.
    history: PlacementHistory,
    /// How far the title logo's rainbow has run, in ticks, wrapping. It only
    /// runs while the terminal has focus.
    title_frames: u32,
    /// Whether the terminal has focus, as it reports. A terminal that does not
    /// report focus never says otherwise, so this starts true and stays so.
    focused: bool,
    /// The terminal's size, which animated backgrounds are sized from as they
    /// tick. Kept current from resize events.
    size: Size,
}

impl App {
    /// Start at the title screen with whatever settings are on disk.
    pub fn new(timing: TimingMode, config: Config, scores: Scores) -> Self {
        Self {
            state: AppState::Title(TitleMenu::default()),
            game: None,
            keymap: config.keymap(),
            menu_keys: config.menu_keymap(),
            held: HeldKeys::new(timing),
            pressed: Vec::new(),
            should_quit: false,
            start_level: config.start_level(config.mode),
            options_origin: OptionsOrigin::Title,
            persist: true,
            background: config.background.create(config.scene),
            background_source: (config.background, config.scene),
            history: PlacementHistory::default(),
            title_frames: 0,
            focused: true,
            // A stand-in until the real terminal reports in; tests keep it.
            size: Size::new(80, 24),
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
        // Nor should the previous run's last clear still be celebrated.
        self.history = PlacementHistory::default();
    }

    /// Persist a settings change and pick up anything that can take effect now.
    fn apply_config(&mut self) {
        // Rebinding applies immediately; the modern timing settings are read when
        // a run starts, so changing them mid-run affects the next one.
        self.keymap = self.config.keymap();
        self.menu_keys = self.config.menu_keymap();
        // Rebuilding re-rolls a random scene, so it must happen only when the
        // background setting itself changed.
        let wanted = (self.config.background, self.config.scene);
        if wanted != self.background_source {
            self.background = self.config.background.create(self.config.scene);
            self.background_source = wanted;
        }
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
        let key = match event {
            Event::Resize(width, height) => {
                self.size = Size::new(width, height);
                return;
            }
            Event::FocusGained => {
                self.focused = true;
                return;
            }
            Event::FocusLost => {
                self.focused = false;
                return;
            }
            Event::Key(key) => key,
            _ => return,
        };

        // Raw mode swallows the terminal's own interrupt, so Ctrl-C is handled
        // here instead: from any screen it leaves, rather than being read as
        // whatever plain `c` happens to be bound to.
        if key.kind != KeyEventKind::Release
            && key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.should_quit = true;
            return;
        }

        if matches!(self.state, AppState::Playing) {
            self.handle_play_key(key, now);
            return;
        }

        match key.kind {
            KeyEventKind::Press => self.handle_menu_key(key),
            // Autorepeat walks a menu the way it scrolls anything else, and edits
            // a name the way it would in any text field, but a held Enter or Esc
            // must not fire twice.
            KeyEventKind::Repeat if self.repeats(&key) => self.handle_menu_key(key),
            KeyEventKind::Repeat => {}
            // A key let go while a menu is up still has to register as released,
            // or it would come back held when play resumes.
            KeyEventKind::Release => {
                if let Some(action) = self.keymap.action_for(&key) {
                    self.held.release(action);
                }
            }
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

    /// Whether holding a key should keep acting on a menu. Movement repeats, as
    /// does editing the high-score name: a held Backspace keeps deleting and a
    /// held letter keeps typing, as in a terminal. Confirming and leaving do not.
    fn repeats(&self, key: &KeyEvent) -> bool {
        if let AppState::GameOver(menu) = &self.state {
            if menu.entering {
                return matches!(key.code, KeyCode::Backspace | KeyCode::Char(_));
            }
        }
        matches!(
            self.menu_keys.input_for(key),
            Some(MenuInput::Up | MenuInput::Down | MenuInput::Left | MenuInput::Right)
        )
    }

    /// Menu screens are driven by the menu keymap rather than the gameplay one —
    /// see the note in `crate::menu`.
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
        match menu.navigate(self.menu_keys.input_for(&key)?)? {
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
            menu.navigate(self.menu_keys.input_for(&key)?, &mut self.config)
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
        view.navigate(self.menu_keys.input_for(&key)?)
            .then(|| AppState::Title(TitleMenu::default()))
    }

    fn pause_key(&mut self, menu: &mut PauseMenu, key: KeyEvent) -> Option<AppState> {
        // The pause key itself resumes, as well as the menu's own Back.
        if self.keymap.action_for(&key) == Some(Action::Pause) {
            return Some(AppState::Playing);
        }

        match menu.navigate(self.menu_keys.input_for(&key)?)? {
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

        match menu.navigate(self.menu_keys.input_for(&key)?)? {
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
                    let events = game.tick(input);
                    // Remembered rather than consumed here: a background reads it
                    // when it next draws, which is after this tick.
                    if events.piece_locked {
                        self.history.record(events.lines_cleared, events.tspin);
                    }
                }
                if self.game.as_ref().is_some_and(Game::is_over) {
                    self.enter_game_over();
                }
            }
        }

        // The background animates on every screen, so the title screen's attract
        // mode runs at the same rate as gameplay.
        let signal = self.signal();
        self.background.tick(TICK, self.size, &signal);

        // Held on its current colours while the player is in another window.
        if self.focused {
            self.title_frames = self.title_frames.wrapping_add(1);
        }
        self.pressed.clear();
    }

    /// How the run is going, for backgrounds that react to it. A quiet default
    /// when there is no run, as on the title screen.
    fn signal(&self) -> PerformanceSignal {
        match &self.game {
            Some(game) => PerformanceSignal::of(game, &self.history),
            None => PerformanceSignal::default(),
        }
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
        let plan = match (&self.state, &self.game) {
            (AppState::Title(_) | AppState::HighScores(_), _) | (_, None) => None,
            (_, Some(game)) => Some(layout::compute(
                area,
                game.preview().len(),
                game.mode().has_hold(),
            )),
        };

        // The background goes down first and keeps clear of wherever the board and
        // its panels are about to be drawn.
        self.draw_background(frame, area, plan.as_ref());

        let board_area = match (plan, &self.game) {
            (Some(plan), Some(game)) => self.draw_play(frame, game, plan),
            _ => None,
        };
        let overlay_area = board_area.unwrap_or(area);
        let visuals = self.config.visuals();

        match &self.state {
            AppState::Playing => {}
            AppState::Title(menu) => menu_ui::render_title(
                frame,
                area,
                menu,
                self.config.mode,
                &visuals,
                self.title_frames,
            ),
            AppState::Options(menu) => menu_ui::render_options(frame, area, menu, &self.config),
            AppState::HighScores(view) => {
                menu_ui::render_scores(frame, area, view, &self.scores, &visuals)
            }
            AppState::Paused(menu) => {
                menu_ui::render_pause(frame, overlay_area, menu, self.config.border)
            }
            AppState::GameOver(menu) => {
                let (score, lines, level) = match &self.game {
                    Some(game) => (game.score(), game.lines(), game.level()),
                    None => (0, 0, 0),
                };
                menu_ui::render_game_over(
                    frame,
                    overlay_area,
                    menu,
                    score,
                    lines,
                    level,
                    self.config.border,
                );
            }
        }

        // Bold is applied as a last pass over everything drawn, rather than in
        // each widget: kitty and most terminals give bold its own heavier face,
        // which is what keeps thin glyphs legible over a busy background.
        for cell in frame.buffer_mut().content.iter_mut() {
            cell.modifier.insert(Modifier::BOLD);
        }
    }

    /// Draws whatever is behind everything else, keeping out of the board and
    /// HUD panels: those widgets paint only the cells they write, so anything
    /// underneath would show through their blank space.
    fn draw_background(&self, frame: &mut Frame, area: Rect, plan: Option<&layout::Layout>) {
        let mut reserved: Vec<Rect> = Vec::new();
        if let Some(plan) = plan {
            // The resize prompt is all there is room for; animation around it
            // would only make it harder to read.
            if plan.tier == layout::Tier::TooSmall {
                return;
            }
            reserved.push(plan.board);
            reserved.extend(plan.panels());
        }

        let signal = self.signal();
        let visuals = self.config.visuals();
        let mut canvas = Canvas::new(frame.buffer_mut(), area, &reserved);
        self.background.render(&mut canvas, &visuals, &signal);

        // Every background dims itself so it sits behind the board. Nothing else
        // is in the buffer yet, so turning that off is a pass over what it drew.
        if !self.config.dim_background {
            for cell in frame.buffer_mut().content.iter_mut() {
                cell.modifier.remove(Modifier::DIM);
            }
        }
    }

    /// Draws the playfield and its panels, returning the board's interior so an
    /// overlay can be centred on it rather than on the whole terminal.
    fn draw_play(&self, frame: &mut Frame, game: &Game, plan: layout::Layout) -> Option<Rect> {
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

        let visuals = self.config.visuals();
        let mut block = visuals.border.apply(
            Block::default()
                .title(format!(" {} ", game.mode().label()))
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        // Whatever the panels had no room for rides along the bottom edge instead,
        // so a narrow terminal still shows how the run is going.
        let missing = match (plan.score, plan.stats) {
            (None, _) => Some(format!(
                " {} · L{} · {}L ",
                game.score(),
                game.level(),
                game.lines()
            )),
            (Some(_), None) => Some(format!(" L{} · {}L ", game.level(), game.lines())),
            _ => None,
        };
        if let Some(line) = missing {
            block = block.title_bottom(
                Line::from(visuals.text(&line).into_owned())
                    .style(Style::default().fg(Color::White))
                    .centered(),
            );
        }
        // The layout keeps a one-cell frame round the field whether or not a
        // border is drawn in it, so the field is taken from inside that frame
        // rather than from the block — which, with no border, would start the
        // field against the frame's left edge, a column off centre.
        let interior = plan.board.inner(Margin::new(1, 1));
        frame.render_widget(block, plan.board);

        board_view::render(frame.buffer_mut(), interior, game, &visuals);

        if let Some(area) = plan.hold {
            hud::render_hold(frame, area, game, &visuals);
        }
        if let Some(area) = plan.next {
            hud::render_next(frame, area, game, &visuals);
        }
        if let Some(area) = plan.stats {
            hud::render_stats(frame, area, game, self.held.mode().label(), &visuals);
        }
        if let Some(area) = plan.score {
            hud::render_score(frame, area, game, &visuals);
        }

        Some(interior)
    }
}

pub fn run(mode: Option<Mode>, start_level: Option<u32>) -> io::Result<()> {
    let precise = supports_keyboard_enhancement().unwrap_or(false);
    let timing = if precise {
        TimingMode::Precise
    } else {
        TimingMode::Inferred
    };

    let mut config = Config::load();
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
    app.size = terminal.size()?;
    let result = event_loop(&mut terminal, &mut app);
    // Settings are saved as they change, but a mode or level given on the
    // command line is only saved here, so the next bare launch resumes it.
    app.save_config();
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
    // Focus reporting is a terminal feature rather than a platform one, and a
    // terminal without it simply never sends the events.
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::cursor::Hide,
        EnableFocusChange
    )?;
    if precise {
        // Real press/release events, which is what makes DAS frame-accurate.
        //
        // Event types alone are not enough: kitty keeps sending plain text keys
        // and Esc in the legacy encoding, which has no release or repeat form, so
        // a letter key would never be released and every Esc repeat or release
        // would arrive as a fresh press. Reporting every key as an escape code
        // fixes both; alternate keys keep shifted letters arriving as capitals.
        stdout.execute(PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
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
    execute!(
        stdout,
        DisableFocusChange,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;
    disable_raw_mode()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background::ClearKind;
    use crate::menu::OptionRow;
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

    /// The title logo's rainbow holds its colours while the terminal is in the
    /// background, and carries on from there when it comes back.
    #[test]
    fn the_title_rainbow_only_runs_while_the_terminal_has_focus() {
        let mut app = title_app();
        let now = Instant::now();
        for _ in 0..10 {
            app.tick(now);
        }
        assert_eq!(app.title_frames, 10);

        app.handle_event(Event::FocusLost, now);
        for _ in 0..10 {
            app.tick(now);
        }
        assert_eq!(app.title_frames, 10, "frozen while unfocused");

        app.handle_event(Event::FocusGained, now);
        app.tick(now);
        assert_eq!(app.title_frames, 11, "resumes where it stopped");
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
    fn backing_out_of_the_title_screen_twice_ends_the_program() {
        let mut app = title_app();
        send(&mut app, KeyCode::Esc);
        assert!(!app.should_quit, "one stray back only points at Quit");
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

        // Back on the title only points at Quit; a second one leaves.
        send(&mut app, KeyCode::Esc);
        assert!(!app.should_quit);
        send(&mut app, KeyCode::Esc);
        assert!(app.should_quit, "then backing out twice leaves the program");
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
            key_event(KeyCode::Char('k'), KeyEventKind::Repeat),
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
        app.handle_event(press(KeyCode::Char('k')), now);
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
            send(&mut app, KeyCode::Char('w'));
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

    /// With the Kitty protocol a held key arrives as repeats, which the name
    /// field has to act on like any terminal would. A held Enter still submits
    /// only once, on its press.
    #[test]
    fn holding_backspace_or_a_letter_repeats_in_the_name_field() {
        let mut app = App::headless(Mode::Modern, 1, TimingMode::Precise);
        let now = Instant::now();
        app.state = AppState::GameOver(GameOverMenu::new(true, "player"));

        app.handle_event(press(KeyCode::Backspace), now);
        for _ in 0..10 {
            app.handle_event(key_event(KeyCode::Backspace, KeyEventKind::Repeat), now);
        }
        app.handle_event(press(KeyCode::Char('a')), now);
        for _ in 0..2 {
            app.handle_event(key_event(KeyCode::Char('a'), KeyEventKind::Repeat), now);
        }
        app.handle_event(key_event(KeyCode::Enter, KeyEventKind::Repeat), now);

        let AppState::GameOver(menu) = &app.state else {
            panic!("still on the game over screen")
        };
        assert_eq!(menu.name, "aaa");
        assert!(menu.entering, "a repeated Enter must not submit");
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

    /// Enter always confirms; `j` does too by default, so one key both rotates
    /// and confirms.
    #[test]
    fn enter_and_the_rotate_key_both_confirm() {
        for code in [KeyCode::Enter, KeyCode::Char('j')] {
            let mut app = title_app();
            send(&mut app, code);
            assert!(matches!(app.state, AppState::Playing), "{code:?}");
        }
    }

    #[test]
    fn a_menu_rebind_takes_effect_immediately() {
        let mut app = title_app();
        app.config
            .set_menu_binding(MenuInput::Down, &[KeyCode::Char('n')]);
        app.apply_config();

        send(&mut app, KeyCode::Char('n'));
        let AppState::Title(menu) = &app.state else {
            panic!("still on the title screen");
        };
        assert_eq!(menu.selected, 1);
    }

    /// In kitty, Esc left the options screen and its release, arriving as a
    /// second Esc press, quit from the title. The cause was the keyboard-protocol
    /// flags in `setup`, which a headless test cannot reach; this pins the app's
    /// half, that a genuine release never acts on a menu.
    #[test]
    fn a_key_release_does_nothing_on_a_menu() {
        let mut app = title_app();
        send(&mut app, KeyCode::Down);
        send(&mut app, KeyCode::Enter);
        assert!(matches!(app.state, AppState::Options(_)));

        let now = Instant::now();
        app.handle_event(press(KeyCode::Esc), now);
        app.handle_event(release(KeyCode::Esc), now);
        assert!(matches!(app.state, AppState::Title(_)));
        assert!(!app.should_quit);
    }

    /// The other half: Esc paused only while it was held, because its release
    /// (and each repeat) arrived as another press. Only the terminal setup can
    /// cause that; this pins that the app itself toggles on presses alone.
    #[test]
    fn pause_is_a_toggle_not_a_hold() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();
        app.handle_event(press(KeyCode::Esc), now);
        app.tick(now);
        assert!(matches!(app.state, AppState::Paused(_)));

        app.handle_event(key_event(KeyCode::Esc, KeyEventKind::Repeat), now);
        app.handle_event(release(KeyCode::Esc), now);
        app.tick(now);
        assert!(matches!(app.state, AppState::Paused(_)), "still paused");

        app.handle_event(press(KeyCode::Esc), now);
        assert!(
            matches!(app.state, AppState::Playing),
            "a second press resumes"
        );
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

    /// The background runs on the title screen too — that is what attract mode is.
    fn render_buffer(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn everything_drawn_is_bold() {
        // "Quit" is an unselected menu item, which is not bold of its own accord.
        let quit_is_bold = |app: &App| {
            let buf = render_buffer(app, 80, 30);
            buf.content
                .iter()
                .find(|cell| cell.symbol() == "Q")
                .expect("the menu is drawn")
                .modifier
                .contains(Modifier::BOLD)
        };
        assert!(quit_is_bold(&title_app()));
    }

    #[test]
    fn background_dimming_can_be_turned_off() {
        let mut app = title_app();
        app.config.background = BackgroundKind::Scene;
        app.config.scene = SceneChoice::Mountains;
        app.apply_config();
        let dimmed = |app: &App| {
            render_buffer(app, 80, 30)
                .content
                .iter()
                .any(|cell| cell.symbol() == "~" && cell.modifier.contains(Modifier::DIM))
        };
        assert!(dimmed(&app), "dimmed by default");

        app.config.dim_background = false;
        assert!(!dimmed(&app));
    }

    #[test]
    fn a_background_draws_behind_the_title_screen() {
        let mut app = title_app();
        app.config.background = BackgroundKind::Scene;
        app.config.scene = SceneChoice::Mountains;
        app.apply_config();

        let rendered = render_to_string(&app, 80, 30);
        println!("{rendered}");
        assert!(rendered.contains('~'), "the scene's horizon is missing");
        assert!(rendered.contains("Play"), "the menu is still on top");
    }

    /// A background that leaked into the playfield or the HUD would make both
    /// harder to read, which is the one thing it must not do.
    #[test]
    fn a_background_never_draws_over_the_board_or_its_panels() {
        use ratatui::backend::TestBackend;

        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        app.config.background = BackgroundKind::DistroLogo;
        app.apply_config();
        for _ in 0..60 {
            app.tick(Instant::now());
        }

        let mut terminal = Terminal::new(TestBackend::new(90, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();

        let plan = layout::compute(Rect::new(0, 0, 90, 30), 1, false);
        let board_interior = Rect::new(
            plan.board.x + 1,
            plan.board.y + 1,
            plan.board.width - 2,
            plan.board.height - 2,
        );
        for y in board_interior.y..board_interior.bottom() {
            for x in board_interior.x..board_interior.right() {
                let symbol = buffer[(x, y)].symbol();
                assert!(
                    symbol == "·" || symbol == " " || symbol == "█",
                    "({x}, {y}) holds {symbol:?}, which is not board content"
                );
            }
        }

        // The stats panel's own text must survive too.
        let rendered = render_to_string(&app, 90, 30);
        println!("{rendered}");
        assert!(rendered.contains("SCORE"));
        assert!(rendered.contains("STATS"));
    }

    /// Rebuilding re-rolls a random scene, so it must happen only when the
    /// background setting actually changed.
    #[test]
    fn the_background_is_rebuilt_only_when_its_setting_changes() {
        let mut app = title_app();
        assert_eq!(app.background.name(), "Blank");

        app.config.background = BackgroundKind::Scene;
        app.apply_config();
        assert_eq!(app.background.name(), "Scene");

        app.config.das_frames += 1;
        app.apply_config();
        assert_eq!(
            app.background.name(),
            "Scene",
            "unchanged by other settings"
        );
    }

    /// Reactive backgrounds read the last clear; it has to be recorded as the
    /// placement happens, since the background only draws afterwards.
    #[test]
    fn the_last_clear_is_recorded_for_reactive_backgrounds() {
        let mut app = App::headless(Mode::Modern, 1, TimingMode::Precise);
        assert_eq!(app.history, PlacementHistory::default());

        // Fill the bottom row everywhere except under the current piece, so one
        // hard drop completes it whatever piece the bag dealt.
        let game = app.game.as_ref().unwrap();
        let piece = game.current().unwrap();
        let cells = game.cells_of(piece);
        let lowest = cells.iter().map(|cell| cell.1).max().unwrap();
        let gap: Vec<i32> = cells
            .iter()
            .filter(|cell| cell.1 == lowest)
            .map(|cell| cell.0)
            .collect();

        let board = app.game.as_mut().unwrap().board_mut();
        let bottom = board.height() as i32 - 1;
        for x in 0..board.width() as i32 {
            if !gap.contains(&x) {
                board.set(x, bottom, Some(crate::engine::piece::PieceKind::O));
            }
        }

        send(&mut app, KeyCode::Char('w'));
        assert_eq!(
            app.history.last_clear,
            ClearKind::Single,
            "a filled row should have registered as a single"
        );
        assert_eq!(app.history.placements, 1, "and counted as a placement");
        assert!(!app.history.last_tspin);
    }

    /// Menus ignore everything but presses, so a release that happens while one is
    /// up has to be routed to the held-key state separately — otherwise a direction
    /// let go during the pause comes back held, and the piece slides on its own.
    #[test]
    fn a_key_released_while_paused_is_not_held_on_resume() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        let now = Instant::now();
        app.handle_event(press(KeyCode::Left), now);
        app.tick(now);
        send(&mut app, KeyCode::Char('p'));
        assert!(matches!(app.state, AppState::Paused(_)));

        app.handle_event(release(KeyCode::Left), now);
        // The pause key resumes directly, without the menu's own clear.
        send(&mut app, KeyCode::Char('p'));
        assert!(matches!(app.state, AppState::Playing));
        assert!(!app.held.is_held(Action::MoveLeft, now));
    }

    /// Holding an arrow scrolls a menu, but a held Enter must not confirm twice.
    #[test]
    fn menu_autorepeat_moves_the_cursor_but_never_confirms() {
        let mut app = title_app();
        let now = Instant::now();
        app.handle_event(key_event(KeyCode::Down, KeyEventKind::Repeat), now);
        let AppState::Title(menu) = &app.state else {
            panic!("still on the title screen");
        };
        assert_eq!(menu.selected, 1);

        app.handle_event(key_event(KeyCode::Enter, KeyEventKind::Repeat), now);
        assert!(
            matches!(app.state, AppState::Title(_)),
            "a repeated Enter must not open Options"
        );
    }

    #[test]
    fn a_new_run_forgets_the_last_runs_final_clear() {
        let mut app = App::headless(Mode::Modern, 1, TimingMode::Precise);
        app.history.record(4, true);
        app.start_run();
        assert_eq!(app.history, PlacementHistory::default());
    }

    /// The narrowest layout has no stats panel, so the score has to go somewhere
    /// else — and putting it there must not cost the board a row, with or without
    /// a drawn border.
    #[test]
    fn a_compact_layout_still_shows_the_score_under_every_border() {
        use crate::ui::style::BorderStyle;

        for border in BorderStyle::ALL {
            let mut app = App::headless(Mode::Modern, 1, TimingMode::Precise);
            app.config.border = border;
            send(&mut app, KeyCode::Char('w'));
            let score = app.game.as_ref().unwrap().score();

            let rendered = render_to_string(&app, layout::MIN_WIDTH, layout::MIN_HEIGHT);
            println!("=== {border:?} ===\n{rendered}");
            assert!(
                // The ASCII border spells the separator plainly.
                rendered.contains(&format!("{score} · L1"))
                    || rendered.contains(&format!("{score} | L1")),
                "{border:?}: no score line"
            );
            let dotted_rows = rendered.lines().filter(|row| row.contains('·')).count();
            assert!(
                dotted_rows >= 20,
                "{border:?}: the board lost rows ({dotted_rows} drawn)"
            );
        }
    }

    #[test]
    fn the_title_screen_draws_without_a_game() {
        let app = title_app();
        let rendered = render_to_string(&app, 80, 26);
        println!("{rendered}");
        assert!(rendered.contains("Play"));
        assert!(!rendered.contains("SCORE"), "no HUD before a run starts");
    }

    /// The ASCII options exist for terminals that cannot be trusted with
    /// Unicode, so with them picked nothing on any screen — menus, board, HUD,
    /// overlays or any background — may draw anything else.
    #[test]
    fn with_the_ascii_options_every_screen_is_pure_ascii() {
        use crate::ui::style::{BorderStyle, Skin};

        let set_up = |app: &mut App, kind: BackgroundKind| {
            app.config.skin = Skin::AsciiBracket;
            app.config.border = BorderStyle::Ascii;
            app.config.background = kind;
            app.apply_config();
            for _ in 0..240 {
                app.tick(Instant::now());
            }
        };
        let check = |app: &App, width: u16, height: u16, screen: &str| {
            let rendered = render_to_string(app, width, height);
            if let Some(ch) = rendered.chars().find(|ch| !ch.is_ascii()) {
                panic!("{screen}: {ch:?} drawn\n{rendered}");
            }
        };

        for kind in BackgroundKind::ALL {
            let mut app = title_app();
            set_up(&mut app, kind);
            check(&app, 80, 30, &format!("title, {kind:?}"));

            app.state = AppState::Options(OptionsMenu {
                rebinding: Some(crate::menu::Rebind::Game(Action::MoveLeft)),
                ..Default::default()
            });
            check(&app, 80, 30, &format!("options, {kind:?}"));

            app.scores.insert(
                Mode::Nes,
                Entry {
                    name: "ada".into(),
                    score: 100,
                    lines: 1,
                    level: 0,
                    date: today(),
                },
            );
            app.state = AppState::HighScores(ScoresView::new(Mode::Nes));
            check(&app, 80, 30, &format!("scores, {kind:?}"));

            for mode in [Mode::Nes, Mode::Modern] {
                let mut app = App::headless(mode, 1, TimingMode::Precise);
                set_up(&mut app, kind);
                // Full, medium and compact layouts.
                for (width, height) in [(100, 30), (45, 26), (22, 22)] {
                    app.size = Size::new(width, height);
                    check(
                        &app,
                        width,
                        height,
                        &format!("{mode:?} {width}x{height}, {kind:?}"),
                    );
                }
                app.state = AppState::Paused(PauseMenu::default());
                check(&app, 100, 30, &format!("pause, {kind:?}"));
                app.state = AppState::GameOver(GameOverMenu::new(true, "x"));
                check(&app, 100, 30, &format!("game over, {kind:?}"));
            }
        }
    }

    /// Taking the border away must not move the field: the frame it sat in is
    /// still reserved, so the field stays centred in it.
    #[test]
    fn the_field_stays_put_whatever_the_border() {
        use crate::ui::style::BorderStyle;

        let first_dot = |border: BorderStyle| {
            let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
            app.config.border = border;
            let rendered = render_to_string(&app, 80, 26);
            // The bottom row of the field is empty dots from edge to edge.
            let row = rendered
                .lines()
                .rev()
                .find(|line| line.contains('·'))
                .unwrap()
                .to_string();
            row.chars().position(|c| c == '·').unwrap()
        };
        assert_eq!(first_dot(BorderStyle::None), first_dot(BorderStyle::Single));
    }

    /// Animated backgrounds size themselves from what the app hands them, so a
    /// resize has to reach it.
    #[test]
    fn a_resize_is_passed_on_to_the_background() {
        let mut app = title_app();
        app.handle_event(Event::Resize(132, 43), Instant::now());
        assert_eq!(app.size, Size::new(132, 43));
    }

    /// Attract mode runs a busy background right up to the title menu; the menu
    /// has to stay readable over it.
    #[test]
    fn the_title_menu_stays_clear_over_an_animated_background() {
        let mut app = title_app();
        app.config.background = BackgroundKind::MatrixRain;
        app.apply_config();
        app.size = Size::new(80, 30);
        for _ in 0..600 {
            app.tick(Instant::now());
        }

        let rendered = render_to_string(&app, 80, 30);
        println!("{rendered}");
        let menu_row = rendered
            .lines()
            .find(|row| row.contains("Options"))
            .expect("the menu is drawn");
        let label = menu_row.find("Options").unwrap();
        // The row's padding either side of the label is clear of rain.
        let around = &menu_row[label.saturating_sub(4)..label + "Options".len() + 4];
        assert_eq!(around.trim(), "Options", "rain in the menu: {around:?}");
    }

    /// Each animated background behind a real game frame, dumped for inspection,
    /// with the HUD checked to have survived it.
    #[test]
    fn each_animated_background_runs_behind_a_game() {
        for kind in [
            BackgroundKind::MatrixRain,
            BackgroundKind::Pipes,
            BackgroundKind::Nyancat,
            BackgroundKind::Bonsai,
            BackgroundKind::Aquarium,
            BackgroundKind::Cowsay,
            BackgroundKind::Locomotive,
        ] {
            let mut app = App::headless(Mode::Modern, 1, TimingMode::Precise);
            app.config.background = kind;
            app.apply_config();
            app.size = Size::new(110, 30);
            // Long enough for the cat to be mid-crossing and the tree grown.
            for _ in 0..60 * 5 {
                app.tick(Instant::now());
            }

            let rendered = render_to_string(&app, 110, 30);
            println!("=== {} ===\n{rendered}", kind.label());
            assert!(rendered.contains("SCORE"), "{kind:?} hid the stats");
            assert!(rendered.contains("HOLD"), "{kind:?} hid the hold panel");
        }
    }

    /// The resize prompt is the one thing on screen when the terminal is too
    /// small; a background animating round it would only obscure it.
    #[test]
    fn no_background_is_drawn_behind_the_resize_prompt() {
        let mut app = App::headless(Mode::Nes, 0, TimingMode::Precise);
        app.config.background = BackgroundKind::Pipes;
        app.apply_config();
        app.size = Size::new(20, 10);
        for _ in 0..300 {
            app.tick(Instant::now());
        }
        let rendered = render_to_string(&app, 20, 10);
        println!("{rendered}");
        assert!(rendered.contains("too small"));
        assert!(!rendered.contains(['─', '│', '┌', '┐', '└', '┘']));
    }
}
