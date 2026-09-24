# tetris-tui — Architecture & Implementation Plan

## Context

You want a playable Tetris game for the Linux terminal, in the spirit of ambient TUI toys you already like (`cava`, `asciiquarium`) but interactive. Two things matter most to you:

1. **Rules accuracy is non-negotiable.** NES-mode and Modern/Arcade-mode (the ruleset shared by Tetris Effect and Puyo Puyo Tetris's Tetris side) each have precise, well-documented official behavior, and you want both modes to play as close to their real references as possible — not "Tetris-flavored," but faithful.
2. **It should feel like a native terminal citizen** — small, minimal-dependency, single binary, works over a resized/transparent terminal — while still being visually rich: color themes that either match your system palette or use official piece colors, multiple ways to draw a tetrimino and the board border, and a genuinely large set of ambient backgrounds behind the board (several inspired by classic Linux terminal toys), one of which reacts to how well you're playing.

This is a from-scratch project (empty directory, no existing code). The plan below is the result of dedicated rules research (cross-checked against harddrop.com's Tetris Wiki and tetris.wiki, the community's authoritative sources for both NES-internals and the modern Guideline) plus architecture design work, incrementally expanded as you added scope over the conversation (backgrounds, title/options screen, visual styles, resizing, transparency, music feasibility).

**Explicit feasibility read, since you asked me to flag anything that's too costly:** nothing requested here is infeasible. The audio corner of the project is the one area worth deliberately de-risking, and per your steer it's now handled like this: **the cava-style background doesn't capture system audio at all — it only visualizes the game's own in-game music/SFX output** (tapping the same PCM buffer being sent to playback, not a PulseAudio/PipeWire monitor source). This eliminates what was the single biggest external-dependency risk in the whole plan (no more "does the user's audio server expose a loopback source" problem). The tradeoff: the visualizer now only exists if the music/SFX system exists, and music/SFX is itself gated on sourcing/composing actual original audio content (real Tetris music can't be bundled for copyright reasons) — so both are bundled together as one **low-priority, optional, last-phase** audio subsystem (§12), off the critical path entirely. Everything else — the ~11 backgrounds, tetrimino skins, border styles, resizing, transparency, rebindable keybinds — is individually cheap once the core rendering pipeline exists.

---

## 1. Crate Choices

### Core (always compiled)
| Crate | Purpose |
|---|---|
| `ratatui` | TUI widget/layout/buffer framework |
| `crossterm` | Terminal backend: raw mode, events, cursor, ANSI/OSC I/O |
| `rand` | Modern-mode 7-bag shuffling + cosmetic background randomness (**not** used for NES's randomizer — see §5.2, NES needs its own hand-rolled algorithm for behavioral accuracy) |
| `serde` + `toml` | Config + high-score persistence, human-editable |
| `dirs` | XDG path resolution (`~/.config/tetris-tui/`, `~/.local/share/tetris-tui/`) |

### Feature-gated (`audio` Cargo feature — off by default, low priority, see §12)
| Crate | Purpose |
|---|---|
| `rodio` (built on `cpal`) | Plays the game's music/SFX; only output, never captures anything from the system |
| `rustfft` | Pure-Rust FFT — turns the *same PCM buffer being played* into bar magnitudes for the cava-style background |

No system audio **capture** anywhere in this design — the cava-style background only visualizes the game's own generated audio (see §8.4), so there's no PulseAudio/PipeWire monitor-source dependency to worry about. This feature is off by default; `cargo build` without it produces a binary with zero audio deps at all, keeping "minimal dependencies" true for the common case.

### Explicitly rejected
- No async runtime — a synchronous fixed-tick loop is simpler and gives cleaner frame-accurate timing; `cpal`'s own callback thread + a small lock-free ring buffer handles audio decoupling.
- No terminal-palette-detection crate initially — OSC 4/10/11 query/parse is ~80 lines over crossterm's raw writer; own it rather than add a dependency for something this small. `terminal-colorsaurus` is an acceptable fallback if the hand-rolled version proves flaky across terminal emulators.
- No ASCII-art-generation crate — title logo and static-scene art are bundled as literal `&str` constants; they need to look specifically good, not be generated.
- No `cowsay`/text-art crate for the reactive background — cow templates + message pools are small authored string data.
- No audio-capture crate/logic at all — deliberately removed in favor of the self-visualizing approach in §8.4/§12.

---

## 2. Project Structure

Single binary crate (module boundaries kept strict enough to split into a workspace later if ever needed):

```
tetris-tui/
  Cargo.toml
  src/
    main.rs                    # terminal init/teardown, panic-hook restore, run App
    app.rs                     # AppState: Title | Options | Playing | Paused | GameOver
    config.rs                  # Config struct, load/save TOML
    highscore.rs               # per-mode high-score table, load/save TOML
    input/
      action.rs                 # Action enum — all game/menu logic reads this, never raw KeyCode
      keymap.rs                  # KeyEvent -> Action map, default bindings, rebind capture + conflict check
      das.rs                      # generic charge/repeat state machine, mode supplies constants
      keyboard_protocol.rs         # Kitty keyboard-protocol (press/release) detection + fallback
    engine/
      board.rs                   # grid, collision, line-clear detection, vanish/buffer zone
      piece.rs                    # tetromino geometry (mode-agnostic shape data)
      game_mode.rs                 # GameMode trait (rotation, randomizer, gravity, scoring, lock/ARE, top-out)
      game_loop.rs                  # fixed-tick driver, mode-agnostic
      state.rs                       # PerformanceSignal snapshot (feeds reactive background)
      nes/
        rotation.rs, randomizer.rs, gravity.rs, scoring.rs
      modern/
        srs.rs, bag.rs, lock_delay.rs, tspin.rs, scoring.rs
    theme/
      palette.rs                 # Theme: OfficialGuideline | SystemAnsi
      detect.rs                   # OSC 4/10/11 query + parse + timeout + 16-color fallback
      skin.rs                      # tetrimino cell-glyph Skin enum (new, see §7)
      border.rs                     # board BorderStyle enum (new, see §7)
    background/
      mod.rs                      # Background trait
      blank.rs, matrix_rain.rs, asciiquarium.rs, static_scene.rs
      cowsay_mood.rs               # reactive, reads PerformanceSignal
      bonsai.rs, pipes.rs, nyancat.rs, locomotive.rs
      fastfetch_logo.rs            # new, see §8.3
      audio_visualizer.rs          # feature-gated, visualizes in-game audio only — see §8.4
    audio/                        # feature-gated (`audio`), shared by music/SFX and the visualizer
      music.rs, sfx.rs, fft.rs      # fft.rs taps the same PCM buffer music.rs/sfx.rs send to rodio
    ui/
      title.rs, options.rs, board_view.rs, hud.rs, pause.rs, game_over.rs, layout.rs
  tests/
    srs_kicks.rs, nes_gravity.rs, nes_scoring.rs, modern_scoring.rs,
    tspin_detection.rs, das_timing.rs
```

### Core trait: `GameMode`
```rust
trait GameMode {
    fn rotate(&self, board: &Board, piece: &ActivePiece, dir: RotationDir) -> Option<ActivePiece>;
    fn spawn_piece(&mut self) -> Piece;
    fn gravity_frames(&self, level: u32) -> u32;
    fn entry_delay_frames(&self, lock_row: u32) -> u32;      // ARE for NES; near-zero for modern
    fn lock_delay(&self) -> Option<LockDelayPolicy>;         // None for NES, Some(..) for modern
    fn score_line_clear(&mut self, lines: u32, level: u32, tspin: TSpinResult, combo: u32, b2b: bool) -> u32;
    fn check_top_out(&self, board: &Board) -> bool;
    fn has_hold(&self) -> bool;
    fn has_ghost(&self) -> bool;
    fn has_hard_drop(&self) -> bool;
}
```
`NesMode` and `ModernMode` each implement this independently — they share `Board`/`Piece` geometry and collision/line-clear detection (identical between rulesets), but rotation, randomizer, gravity, lock behavior, and scoring are **fully separate**, never a shared "rotate with optional kicks"-style function. This is deliberate: forcing the two rulesets through one shared implementation risks subtly wrong behavior in both, which conflicts directly with your top priority.

---

## 3. NES Mode — Researched Rule Values

Cross-checked against harddrop.com's Tetris Wiki and tetris.wiki (community-authoritative, ROM-derived).

### 3.1 Gravity (frames/row, NTSC 60fps)
| Level | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10–12 | 13–15 | 16–18 | 19–28 | 29+ |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Frames/row | 48 | 43 | 38 | 33 | 28 | 23 | 18 | 13 | 8 | 6 | 5 | 4 | 3 | 2 | 1 |

Not a clean arithmetic progression (note the 8→6 irregularity at levels 8–9) — hardcode as a const table, not a formula. Level 29+ = the "kill screen" speed. PAL timing is out of scope for v1.

### 3.2 Piece randomizer — NOT 7-bag
1. Roll 1: value 0–7 (7 pieces + 1 unused "dummy" slot).
2. Accept roll 1 only if it's neither the previous piece **nor** the dummy value.
3. Otherwise, Roll 2: value 0–6, dealt **unconditionally** — this roll is allowed to repeat the previous piece.

This is the source of NES Tetris's documented long droughts (13+, occasionally 50–60 pieces without a given piece) — a real, intentional-to-replicate flaw, qualitatively different from 7-bag's hard 12-piece-max-wait guarantee. Model as an explicit state machine (`last_piece` + LFSR-ish generator), not `rand`'s shuffle — the statistical *bias signature* is the gameplay-relevant behavior. **Confidence flag**: the roll1/roll2 mechanic itself is well-sourced; the exact underlying LFSR polynomial/seed is lower-confidence from this research pass — verify against a primary disassembly source (e.g. the fractal161 NES-Tetris-RNG paper, or a known open disassembly) during implementation before calling it bit-exact. Frame-tied RNG advancement (needed for speedrunner-style "RNG manipulation") is a stretch goal, not required for "plays like NES Tetris."

### 3.3 Rotation — Nintendo Rotation System (NRS)
- **No wall kicks** — a colliding rotation is simply rejected, piece unchanged.
- No hold, no ghost piece, no soft-drop lock (holding down only speeds gravity, doesn't force a lock).
- O: 1 state. I, S, Z: 2 states. J, L, T: 4 states, pivoting on the center cell.
- Spawn: highest block on visible row 20 ("pops in" partially visible), I/O centered, others per the classic NES spawn table.

### 3.4 DAS / ARE
- Initial press: instant shift. Then **16 frames** charge before first auto-repeat.
- After the first auto-repeat, counter resets to **10** (not 0) → subsequent repeats every **6 frames** (~10 shifts/sec).
- **DAS charge persists through ARE** (doesn't reset during entry delay) — lets a held direction "buffer" so the next piece auto-shifts immediately. Model DAS state as independent of piece lifecycle, reset only on key release/direction change.
- **ARE**: 10–18 frames, banded by lock height — bottom two rows = 10 frames, +2 per 4-row band upward, capped ~18. **Confidence flag**: the banding pattern is consistently sourced; precise row cutoffs need confirmation against a primary source during implementation.
- Line-clear flash delay: ~17–20 frames, parity-dependent. **Confidence flag**: implement as a flat ~18-frame placeholder, refine if frame-perfect accuracy is later demanded.
- No hard drop exists in NES mode — omit entirely from input/scoring.

### 3.5 Top-out
**Lock-based**, not spawn-based: a piece may spawn overlapping the stack; the player can still move/rotate out. Game over triggers only when a piece **locks** while still overlapping, or truly cannot be placed. This is meaningfully looser than Modern mode's top-out and must be its own `GameMode` method, not shared.

### 3.6 Scoring
`score += base(lines) × (level + 1)`, base: Single=40, Double=100, Triple=300, Tetris=1200. Soft drop: **+1/cell**, not level-scaled.

### 3.7 Level-up thresholds
From `start_level`, advance at whichever comes first: `start_level×10 + 10` lines, or `max(100, start_level×10 − 50)` lines. After the first level-up, every flat 10 additional lines advances one level.

---

## 4. Modern/Guideline Mode — Researched Rule Values

### 4.1 SRS rotation + kick tables
O: no rotation/kicks. All others spawn horizontally in the buffer zone; I/O centered, J/L/T/S/Z flat-side-first, slightly left of center.

**JLSTZ kicks** (try in order, first non-colliding wins):

| Transition | Test1 | Test2 | Test3 | Test4 | Test5 |
|---|---|---|---|---|---|
| 0→R | (0,0) | (−1,0) | (−1,+1) | (0,−2) | (−1,−2) |
| R→0 | (0,0) | (+1,0) | (+1,−1) | (0,+2) | (+1,+2) |
| R→2 | (0,0) | (+1,0) | (+1,−1) | (0,+2) | (+1,+2) |
| 2→R | (0,0) | (−1,0) | (−1,+1) | (0,−2) | (−1,−2) |
| 2→L | (0,0) | (+1,0) | (+1,+1) | (0,−2) | (+1,−2) |
| L→2 | (0,0) | (−1,0) | (−1,−1) | (0,+2) | (−1,+2) |
| L→0 | (0,0) | (−1,0) | (−1,−1) | (0,+2) | (−1,+2) |
| 0→L | (0,0) | (+1,0) | (+1,+1) | (0,−2) | (+1,−2) |

**I-piece kicks** (distinct table):

| Transition | Test1 | Test2 | Test3 | Test4 | Test5 |
|---|---|---|---|---|---|
| 0→R | (0,0) | (−2,0) | (+1,0) | (−2,−1) | (+1,+2) |
| R→0 | (0,0) | (+2,0) | (−1,0) | (+2,+1) | (−1,−2) |
| R→2 | (0,0) | (−1,0) | (+2,0) | (−1,+2) | (+2,−1) |
| 2→R | (0,0) | (+1,0) | (−2,0) | (+1,−2) | (−2,+1) |
| 2→L | (0,0) | (+2,0) | (−1,0) | (+2,+1) | (−1,−2) |
| L→2 | (0,0) | (−2,0) | (+1,0) | (−2,−1) | (+1,+2) |
| L→0 | (0,0) | (+1,0) | (−2,0) | (+1,−2) | (−2,+1) |
| 0→L | (0,0) | (−1,0) | (+2,0) | (−1,+2) | (+2,−1) |

**Verification required at implementation time**: the published y-offset sign convention must be validated against our own row-increases-downward board coordinates using known scenarios (e.g. the classic TST kick, an I-piece flat kick) with pinned unit tests — don't trust the transcription blindly (§9).

### 4.2 T-spin / mini T-spin detection
1. Last placement action must be a rotation (not a shift/drop).
2. Of the T's 4 diagonal corners (3×3 box), ≥3 occupied (stack or wall/floor) to register any T-spin.
3. **Full** if both "front" corners (adjacent to the open/pointing side) are occupied, regardless of back corners. **Mini** if only one front corner occupied and both back corners occupied.
4. **5th-kick override**: if the successful rotation used kick test index 5, promote to **full** regardless of corner geometry (the "TST kick" case).
5. Wall/floor cells count as occupied.

### 4.3 Randomizer — 7-bag
Fisher-Yates shuffle of all 7 pieces per bag (via `rand::seq::SliceRandom`), refill when exhausted. Guarantees ≤12-piece max wait between repeats of any piece. **Confidence flag**: the common convention that the very first bag of a game is constrained so its first piece is never S, Z, or O (avoiding a forced overhang on move one) is widely cited in the community but wasn't confirmed against a single authoritative primary source in this research pass — confirm against tetris.wiki's Random Generator page before finalizing bag-seeding logic (§14).

### 4.4 Lock delay
500ms (30 ticks @60Hz) after a piece can't fall further. Move/rotate resets the timer, **capped at 15 resets** (modern "Extended Placement Lock Down," not old unlimited "Infinity"). A move to a **new lowest row** resets the counter regardless of the 15-cap. Hard drop bypasses lock delay entirely.

### 4.5 Scoring
Base × level (level starts at 1 for this formula):

| Action | Points |
|---|---|
| Single / Double / Triple / Tetris | 100 / 300 / 500 / 800 |
| Mini T-spin (0 lines) / T-spin (0 lines) | 100 / 400 |
| Mini T-spin Single / T-spin Single | 200 / 800 |
| Mini T-spin Double / T-spin Double | 400 / 1200 |
| T-spin Triple | 1600 |

- **Back-to-back**: ×1.5 on any Tetris or T-spin-with-lines that immediately follows another such clear. A 0-line T-spin doesn't break the chain; a plain Single/Double/Triple does.
- **Combo**: `50 × combo_count × level`, combo increments per consecutive clearing placement, resets on a non-clearing one. **Confidence flag**: sources disagree slightly on the exact starting index (does the 2nd consecutive clear score `50×1×level` or `50×0×level`?) — pin this against a reference implementation before finalizing (§14).
- Soft drop 1pt/cell, hard drop 2pt/cell (not level-scaled).
- **Perfect clear**: Single=800×lvl, Double=1200×lvl, Triple=1800×lvl, Tetris=2000×lvl, B2B-Tetris-PC=3200×lvl, additive on top of line score.

### 4.6 DAS/ARR — configurable, not fixed
Guideline doesn't mandate exact values. Default DAS ≈133ms (8 frames), ARR ≈33ms (2 frames), both **user-configurable** in the options screen (unlike NES's fixed values, which must **not** be exposed as tunable — that would silently break NES-mode accuracy; the options-screen asymmetry between modes here is deliberate).

### 4.7 Top-out — stricter than NES
Triggers on: spawn cells already occupied ("block out"), a piece locking entirely above the visible field ("lock out"), or a block pushed above the buffer ceiling. Separate `GameMode` method from NES's lock-based rule.

---

## 5. Rendering, Legibility, Resize, Transparency

- **2 terminal columns per game cell** (standard trick for square-looking blocks given typical ~1:2 font aspect ratio).
- **Layering, back to front**: full-screen background → opaque board panel (playfield + border) → HUD panels → active/ghost piece → overlay UI (pause/game-over as solid bordered boxes — no real terminal alpha blending exists, so "semi-opaque" dialogs are simulated with a solid fill, not literal transparency).
- **Legibility**: the board panel is always fully opaque; backgrounds are only ever visible in surrounding margins/chrome, never bleeding into the playfield. Simplest robust approach, no per-background dimming math needed.
- **Responsive resizing (btop-style)**: layout recomputed every frame from current terminal size via ratatui `Layout`/`Constraint`s, not a fixed assumed size.
  - Fixed logical grid (10×20 NES / 10×20+buffer modern) but terminal-cell size of each game cell and chrome scales with available space.
  - Side panels (hold, next-queue, score/stats, background canvas) **progressively collapse** as space shrinks — e.g. drop the background layer first, then the next-piece queue length, then hold/score panel labels to abbreviated form — before falling back to a minimum-size message, mirroring how btop hides panels rather than breaking layout.
  - Below a defined minimum (board's minimum cell footprint + a thin HUD strip, e.g. roughly 10×2+2 cols × 20+2 rows plus a stats line), show a centered "resize your terminal" message instead of a squeezed board.
  - Backgrounds receive the actual available `Rect` each tick and must adapt their canvas to it (e.g. matrix-rain column count, pipe cursor bounds) rather than assuming a fixed size.
- **HUD arrangement, shared by both modes**: hold top-left, next queue top-right, stats bottom-left, score bottom-right. Top panels align with the board's top edge, bottom panels with its bottom edge. Hold top-left and the queue on the right follow Guideline games; stats on the left follows NES. A panel for a feature the ruleset lacks (NES hold) is left out, not drawn empty, and nothing else moves into its place. The next box is sized to the chosen preview count: three rows per piece, so six previews plus the one-line score box exactly fill the board's 22 rows.
- **Transparency**: never paint an explicit opaque RGB background fill for cells that should read as "empty terminal" — use the terminal's default/reset background so the terminal emulator's own transparency/compositor blur shows through, same principle as a transparent-background btop/fastfetch setup.
  - `blank` background = literally untouched/transparent, not an opaque fill.
  - All animated/cosmetic backgrounds only ever set **foreground** character+color on their "off"/empty cells, never an opaque cell background — transparency is preserved everywhere except the board panel, which intentionally stays opaque for legibility (per the layering rule above; no separate toggle needed, it's an inherent property of the board panel, not a background property).
  - Caveat: true transparency is a terminal-emulator/compositor feature — the app's job is only to avoid fighting it by never forcing opaque fills where undesired.

---

## 6. Input Handling

- **Fixed-tick loop (60Hz)**, not purely event-driven — matches NES's native frame rate for clean integer frame counting, and gives modern-mode's ms-based lock delay a consistent tick quantum too. Each tick: drain crossterm events → advance DAS/ARR state → advance gravity/lock-delay/ARE → apply resulting movement/lock/clear → render.
- **Action-indirection layer, required for rebinding** (per your title-screen/options request): game logic never matches raw `KeyCode`. `Action` enum (`MoveLeft`, `MoveRight`, `SoftDrop`, `HardDrop`, `RotateCw`, `RotateCcw`, `Hold`, `Pause`, `UiUp/Down/Confirm/Cancel`, ...) resolved through a `Keymap: HashMap<KeyEvent, Action>`. The options screen's keybind submenu: select an action → capture next key → check conflicts against existing bindings → confirm/cancel → persist to config.
- **Real risk, called out explicitly**: terminals don't natively expose key-down/key-up the way DAS timing needs — most only send repeated `Press` events at the OS's auto-repeat rate. Detect and prefer the **Kitty keyboard protocol** (`crossterm::event::PushKeyboardEnhancementFlags(REPORT_EVENT_TYPES)`) on terminals that support it (kitty, wezterm, foot, iTerm2, newer Alacritty) for true press/release and frame-accurate DAS. Elsewhere (plain xterm, many tmux/screen sessions, GNOME Terminal/VTE) fall back to a heuristic: treat a key as "held" while repeat presses keep arriving within a short window, "released" after a timeout gap — inherently approximate. Surface this to the user (a startup capability probe reporting "precise" vs "approximate" input timing) and document a terminal compatibility matrix in the README.
- `DasProfile` (charge/repeat frame counts) supplied per mode — NES's fixed §3.4 values, modern's configurable §4.6 values.

---

## 7. Visual Styles: Color Theme, Tetrimino Skin, Border Style

Three independent, composable axes — color theme, cell-rendering skin, and board border — each its own enum/trait, not merged, so any combination works.

### 7.1 Color Theme
```rust
enum Theme { OfficialGuideline, SystemAnsi(AnsiPalette) }
```
- `OfficialGuideline`: fixed piece colors (I=cyan, O=yellow, T=purple, S=green, Z=red, J=blue, L=orange).
- `SystemAnsi`: query the terminal's real palette via OSC 10/11 (fg/bg) and OSC 4;n (16 indexed slots) with a short (~100ms) timeout per query; parse `rgb:RRRR/GGGG/BBBB`. On timeout/unsupported terminal (SSH relays, tmux/screen blocking OSC, plain Linux console) fall back to symbolic 16-color ANSI codes (`Color::Red` etc.) rather than resolved RGB — still matches the user's theme since the terminal renders its own configured palette for those codes. Map the 7 pieces onto distinct/well-separated slots (not a naive 1:1 index scheme) so pieces stay visually distinguishable under arbitrary real-world palettes. Detected once per session on selection, not per-frame.

### 7.2 Tetrimino Skin (new — cell-glyph rendering, independent of color)
```rust
enum Skin { SolidBlock, Shaded, AsciiBracket, LetterPerCell }
```
- **Solid block** (default): full-block character (`█`) per cell, colored per theme.
- **Shaded**: partial-block shade characters (`▓`/`▒`/`░`) for a softer/retro texture.
- **ASCII bracket**: `[]`-per-cell — plain-ASCII, max-compatibility fallback for terminals with poor Unicode/color support.
- **Letter-per-cell**: each cell shows its piece letter (I/O/T/S/Z/J/L) — a nod to old terminal Tetris clones that couldn't rely on color at all; useful as a colorblind-friendly / no-color-terminal option.
- Caveat: every skin must preserve the 2-columns-per-cell alignment; some skins (letter) read better with the ghost piece rendered distinctly dimmed/hollow rather than the same glyph half-opacity (terminals can't do real alpha) — use a different glyph or dim color for ghost cells regardless of skin.

### 7.3 Board Border Style (new)
```rust
enum BorderStyle { None, Ascii, Single, Double, Rounded, Heavy }
```
Plain ASCII (`+--+`/`|`) through single/double/rounded/heavy Unicode box-drawing, or no border for minimal chrome. A heavier border is a reasonable default when a busy animated background is active (keeps the board's edge readable against motion behind it) — expose this as a sensible default pairing, not a hard rule; both remain independently selectable in the options screen.

All three (theme, skin, border) get their own options-screen rows and their own config fields (§10).

---

## 8. Background System — 11 types

```rust
struct PerformanceSignal {
    combo_count: u32,
    back_to_back_active: bool,
    last_clear_kind: Option<ClearKind>,
    stack_height_fraction: f32,   // 0.0 empty .. 1.0 near top-out
    score: u64,
    level: u32,
    is_game_over: bool,
}

trait Background {
    fn tick(&mut self, dt: Duration);
    fn render(&self, buf: &mut Buffer, area: Rect, board_rect: Rect, theme: &Theme, signals: &PerformanceSignal);
    fn name(&self) -> &'static str;
}
```
One uniform trait signature — `signals` is handed to every background each tick, purely cosmetic ones simply ignore it; no special-cased second trait or downcasting needed for the one reactive background.

| # | Background | Nature | Notes |
|---|---|---|---|
| 1 | Blank | static, transparent no-op | literally untouched terminal, per §5 transparency rule |
| 2 | Matrix rain | animated, cosmetic | falling glyph columns, self-contained PRNG |
| 3 | Asciiquarium-style | animated, cosmetic | drifting fish/creature sprites, reimplemented natively (no dependency on the real Perl tool) |
| 4 | Cowsay mood | **animated + reactive** | reads `PerformanceSignal`; see §8.1 |
| 5 | Static scenes | static after first draw | bundled fixed art; see §8.2 |
| 6 | Bonsai (cbonsai-style) | animated, cosmetic | procedural recursive-branch growth, calm |
| 7 | Pipes (pipes.sh-style) | animated, cosmetic | box-drawing cursors laying pipe glyphs |
| 8 | Nyancat | animated, cosmetic | fixed-frame sprite loop + scrolling rainbow trail |
| 9 | Locomotive (sl-style) | animated, cosmetic, **rare event** | occasional cross-screen chug; doubles nicely as a game-over easter egg |
| 10 | Fastfetch logo | static or slow-drift | your distro's ASCII logo tiled across the background; see §8.3 |
| 11 | Cava-style audio bars | animated, cosmetic, **feature-gated, low priority** | see §8.4 — visualizes the game's *own* music/SFX only, not system audio; exists only if §12's audio subsystem gets built |

**Considered and rejected**: `genact`/`hollywood`-style fake scrolling terminal logs — the most visually "busy"/reading-shaped of the candidates considered, which cuts against "background must not distract from the board" harder than the others. Left out of v1 scope; easy to revisit later if the chosen set feels too quiet.

### 8.1 Cowsay mood — the reactive one
Small bundled set of ASCII cow templates + message pools (authored `&str` constants, no dependency on the real `cowsay` binary), selected by a debounced rule table read from `PerformanceSignal` (switch mood only on a materially-changed signal + a minimum ~1–2s dwell, to avoid flicker):
- `stack_height_fraction > ~0.8` → nervous/panicked line.
- Fresh Tetris or T-spin clear → celebratory line, alternate pose.
- `combo_count >= 3` → encouraging streak line.
- `back_to_back_active` → smug/confident variant.
- `is_game_over` → dedicated game-over line.
- Idle default → slow-rotating neutral filler pool.

### 8.2 Static scenes
5–8 bundled fixed ASCII/Unicode scenes as `&str` constants. Default: random-per-session on activation; options screen also offers explicit left/right cycling through the set. `tick()` is a true no-op; `render()` redraws the same content harmlessly (cheap).

### 8.3 Fastfetch logo (new)
Tiled repetition of the small ASCII/Unicode distro logo fastfetch shows — naturally unique per user's distro.
- **Recommended approach**: parse `/etc/os-release`'s `ID`/`ID_LIKE` ourselves and bundle a small set of common distro logos (Arch, Debian, Ubuntu, Fedora, NixOS, openSUSE, Manjaro, + a generic Tux/penguin fallback) as our own `&str` art constants. No external process dependency, consistent with how the other novelty backgrounds are self-reimplemented rather than shelled out to.
- **Alternative considered**: shell out to the user's installed `fastfetch` binary for perfect fidelity to their actual configured logo, including custom ones. Rejected as the *primary* path because it's the one background with an external **binary** dependency plus CLI-flag fragility. **Confidence flag**: fastfetch's `--logo`/`--logo-type`/`--logo-width` flags are confirmed to exist and control logo display, but the exact "print only the ASCII logo block, no info modules" invocation needs a quick `fastfetch --help`/config check at implementation time before relying on it — not something to guess at from memory. Worth a follow-up flag/toggle later ("use installed fastfetch if present") behind the same graceful-degradation pattern as the audio background, not required for v1.
- Rendering: tile the (possibly multi-line, possibly colored) logo block across the background area; treat as static (like static scenes) rather than animated, since a repeated logo doesn't need motion to read well.

### 8.4 Cava-style audio bars — low priority, feature-gated, visualizes in-game audio only
Per your steer: this is **not** a system-audio-capture background. It only visualizes whatever the game's own music/SFX subsystem (§12) is currently playing, by tapping the same PCM buffer `audio/music.rs`/`sfx.rs` hand to `rodio` and running it through `rustfft` for bar magnitudes on a rolling window.
- No PulseAudio/PipeWire monitor source, no device enumeration, no "is there a loopback available" problem — the one external-environment risk that made the original cava idea the plan's riskiest item is gone by construction.
- Direct consequence: this background **only exists if the `audio` feature is built and music/SFX is actually configured/playing** (§12). With no music system built or no track selected, it has nothing to visualize — show it as unavailable/greyed in the options screen exactly like any other feature that needs a prerequisite, and fall back to an idle "flatline"/breathing animation rather than erroring if it's selected with nothing currently playing.
- Because it's entirely internal (no external device, no failure modes beyond "nothing is playing"), it needs no separate graceful-degradation engineering beyond that idle-state fallback — simpler to build correctly than the original system-capture design, once §12 exists at all.

---

## 9. Menu/UX Flow

```
[Title Screen] — logo + a live Background running in "attract mode" (no board panel yet)
   "Press Enter" / "O for Options" / "Q to quit"
        │  Confirm
        ▼
[Options / Settings Screen]
   Game Mode: NES | Modern
   Background: one of the 11 (§8), with description text
   Color theme: Official | System-detected ANSI (§7.1)
   Tetrimino skin: Solid | Shaded | ASCII bracket | Letters (§7.2)
   Board border: None | ASCII | Single | Double | Rounded | Heavy (§7.3)
   Keybinds: per-Action rebind list with conflict detection (§6)
   DAS/ARR tuning: Modern mode only (NES's is fixed, not exposed — deliberate asymmetry, §4.6)
   Next pieces: Modern mode only, 1–6, default 5 (NES always previews one). Guideline titles disagree — Puyo Puyo Tetris 5, Tetris 99 and Tetris DS 6, Tetris Worlds 3 (GBA) or 6, TETR.IO 1–6 selectable, Jstris 0–5 selectable (tetris.wiki game pages); the Guideline itself only says "up to six"
   Starting level
   High scores: view-only, separate NES/Modern lists
   All changes persist to config.toml immediately on change
        │  Start Game
        ▼
[Playing]  ⇄  [Paused] (Resume / Options / Quit to Title)
        │  top-out
        ▼
[Game Over / High Score]  — summary, name-entry if a new per-mode top-10, Retry / Back to Title
        │
        ▼
[Title Screen]
```
Driven by an `AppState` enum in `app.rs`, run through the same fixed-tick loop as gameplay so the attract-mode background animates smoothly and consistently.

---

## 10. Config / Persistence

- `~/.config/tetris-tui/config.toml`: game mode, background choice (+ per-background sub-choice, e.g. static-scene selection), theme, skin, border style, keybindings, modern DAS/ARR/ghost, starting level.
- `~/.local/share/tetris-tui/scores.toml`: two separate top-10 lists (NES, Modern — not comparable, kept apart), each `{ name, score, lines, level, date }`.
- Both plain `serde`+`toml`, human-editable; a missing/corrupt file falls back to defaults rather than blocking launch.

---

## 11. Verification / Testing Strategy

**Unit tests** (`cargo test`, deterministic):
- SRS kick tables: const-table transcription checks + scenario tests (wall/floor placements, a known TST setup validating the 5th-kick T-spin promotion) — don't trust the sign-convention transcription blindly (§4.1).
- T-spin detection: fixture boards for a clear full, a clear mini, and the kick-5-override case.
- NES gravity/scoring: full table from §3.1 level-by-level; level-up thresholds at a few starting levels; scoring formula across level/line combinations.
- Modern scoring: base values, B2B application/non-application (0-line T-spin doesn't break the chain), combo formula, perfect-clear stacking.
- DAS timing: simulate held-key ticks, assert NES's 16-then-6 cadence and the "charge persists through ARE" behavior precisely.
- NES randomizer: test the *logic path* deterministically (force a same-value roll1 → assert reroll; assert roll2 always accepted even on repeat) plus a statistical sanity check that repeats can occur within a short window (regression guard against someone swapping in a 7-bag-shaped implementation by mistake).
- 7-bag: property test — any 7 consecutive draws aligned to a bag boundary are a full permutation; max gap ≤12.

**Manual/integration verification**:
- Cross-check computed NES frame counts (gravity/ARE/DAS) against published frame-data/speedrunning references at a few known points before finalizing the ARE row-boundary and line-clear-delay constants flagged uncertain in §3.4.
- Terminal compatibility matrix: verify DAS precision (Kitty protocol vs. fallback heuristic) across kitty, wezterm, foot, Alacritty, GNOME Terminal/VTE, plain xterm/tmux — ship the results in the README.
- OSC 4/10/11 detection: verify against a few real terminal emulators plus the tmux/screen-blocks-OSC fallback path.
- Audio background: test both with and without an active monitor source, confirming no panic/hang either way.
- A few T-spin setups compared against a known-good reference (e.g. a documented TETR.IO/jstris scenario), since the corner-rule edge cases are the trickiest single piece of Guideline logic to get right from a spec document alone.

---

## 12. Audio Subsystem (Music/SFX + Cava-style Visualizer) — Feasibility Assessment, Low Priority

Both bundled together deliberately, per your steer: the cava-style background (§8.4) only visualizes what this subsystem plays, so there's no separate "audio capture" feasibility question anymore — just this one. Kept explicitly out of the core milestone path and behind the `audio` Cargo feature (§1), off by default.

- **Mechanism**: `rodio` (built on `cpal`) for playback only — no capture code anywhere. `audio/fft.rs` taps the same PCM stream `music.rs`/`sfx.rs` send to `rodio` and runs `rustfft` over it for the visualizer; both consumers (speakers + bar visualizer) read from one source of truth, so there's nothing to keep in sync.
- **Content, and the real blocker**: NES mode traditionally offers 3 selectable tracks ("Type A/Korobeiniki," "B," "C") with a tempo bump at higher levels in some versions; modern titles pair generic music with per-action SFX (move/rotate/lock/clear/tetris-fanfare/level-up/game-over/menu-nav). This is a **content-sourcing problem, not a coding problem**: the Korobeiniki melody itself is public-domain (Russian folk tune), so an original arrangement is safe to bundle, but any specific existing recording/arrangement (e.g. lifted from an actual ROM or commercial release) carries that arranger's/publisher's copyright and must not be bundled. Nothing here ships until suitable original/CC0 audio actually exists.
- **Binary size**: even compressed OGG assets add real bytes to what's meant to be a small single binary. Recommendation: treat audio assets as optional external files under the config directory (opt-in drop-in) rather than `include_bytes!`-embedded in the base binary, keeping the default build small.
- **Placement**: a clearly optional, lowest-priority stretch phase after core gameplay, all 10 non-audio backgrounds, and theming are solid — should never compete with or delay the rules-accuracy work that's this project's actual point. The options screen should treat both music and the cava-visualizer as greyed-out/unavailable until this phase exists at all.

---

## 13. Milestone / Phase Breakdown

Ordered to de-risk rules-accuracy first, cosmetics last.

| Phase | Scope |
|---|---|
| 0 | Copy this plan into the project root as `brief.md` (project reference doc), then: module skeleton, `Config`/`Keymap` defaults (no persistence yet), terminal raw-mode/alt-screen init+teardown. |
| 1 | Mode-agnostic engine core: `Board`, `Piece` geometry, collision, line-clear detection, `GameMode` trait shape, headless fixed-tick loop (unit-testable without rendering). |
| 2 | **NES mode, full rules** (§3): rotation, randomizer, gravity, DAS/ARE, scoring, lock-based top-out. Full §11 unit-test suite green before proceeding. |
| 3 | Minimal rendering + input, NES playable end-to-end: board widget (2-col/cell), basic HUD, `Action`/`Keymap` input wired up, Kitty-protocol detection+fallback, blank background only. Manual side-by-side accuracy check per §11. |
| 4 | **Modern mode, full rules** (§4): SRS+kicks, 7-bag, hold, ghost, lock delay+reset cap, T-spin detection, full scoring, stricter top-out, configurable DAS/ARR. Full §11 unit-test suite green. |
| 5 | Menu/UX + persistence: `AppState` machine, Title, Options (mode + keybind + DAS/ARR sections functional even before full theme/background variety exists), Pause, Game Over/high-score flow, config/highscore TOML. |
| 6 | Theming: `Theme` (official + system-ANSI detection/fallback), `Skin`, `BorderStyle` (§7) — wired into rendering and the Options screen. |
| 7 | All 10 non-audio backgrounds, cheapest first: (a) Blank + Static scenes + Fastfetch logo (all static/near-static, bundled assets); (b) procedural cosmetic animations — Matrix rain, Pipes, Nyancat, Bonsai — self-contained, any order; (c) Asciiquarium (most involved purely-cosmetic one); (d) Cowsay mood — first to need the `PerformanceSignal` plumbing from game loop to render layer, its own step; (e) Locomotive — rare-event timer/probability trigger + optional game-over tie-in. Cava-style bars deliberately excluded from this phase — see Phase 9. |
| 8 | Polish/hardening: resize edge cases incl. progressive panel collapse (§5), transparency verification (§5), config-corruption fallback hardening, clippy/fmt, README (build instructions incl. default no-`audio` build, terminal compatibility matrix from §11), final manual accuracy pass on both modes. **This is the practical "done" point** — everything through here is core scope. |
| 9 (stretch, optional, lowest priority) | Audio subsystem per §12, only once appropriate original/CC0 content exists: 9a) music/SFX playback via `rodio`; 9b) Cava-style visualizer (§8.4), built only after 9a since it has nothing to visualize without it. |

---

## 14. Flagged Uncertainties (verify against primary sources during implementation, not blind-trusted from this research pass)

### Resolved during implementation

- **NES randomiser, fully pinned.** The roll1/roll2 description was replaced with the ROM's actual algorithm from fractal161's *NES Tetris RNG* §3.1–3.2, cross-checked against meatfighter's disassembly writeup: a 16-bit Fibonacci LFSR seeded 0x8988 (feedback = bit1 XOR bit9), first roll = high byte + session spawn count mod 8, rerolling on 7-or-repeat with `(high byte & 7) + previous piece's spawn orientation ID` mod 7. Both sources independently agree on the spawn orientation IDs, and the RNG paper's "orientation ID mod 7" table (T=2, J=0, Z=1, O=3, S=4, L=0, I=4) is asserted in `rotation.rs` tests. The LFSR period (32767) and the paper's worked example are also pinned by tests.
- **NRS geometry.** Derived from the ROM's 19-orientation table at $8A9C, with the one orientation quoted verbatim in the source (Td) asserted directly, plus structural tests: 19 orientations total, state counts 4/4/4/2/2/2/1, and every successive state a true clockwise rotation.
- **SRS kick-table sign convention** (§4.1) — resolved. Published offsets grow y upward; the board's rows grow downward, so the sign is flipped in exactly one function. Pinned by two real-board scenarios: a vertical I against the left wall kicking right via test 3, and a flat I on the floor kicking *up* two rows via test 5.
- **7-bag first piece** (§4.3) — resolved. The Guideline places no constraint on the first piece; the "never S/Z/O first" rule belongs to Tetris The Grand Master Ace, not the Guideline (tetris.wiki/Random_Generator). Implemented as a plain shuffle. The documented guarantees (max 12-piece gap, S/Z runs bounded at 4) are asserted as property tests.
- **fastfetch logo invocation** (§8.3) — not needed. The logo background took §8.3's recommended route of bundling its own distro logos, matched from `/etc/os-release`, so nothing ever calls `fastfetch`.
- **NES line-clear delay** (§3.4) — resolved from the disassembly (CelestialAmber/TetrisNESDisasm, `updateLineClearingAnimation`). Five steps, each blanking one column either side of centre (the `leftColumns` 4,3,2,1,0 / `rightColumns` 5,6,7,8,9 tables), advancing only on frames where `frameCounter & 3 == 0`. So the first step lands 1–4 frames after the lock and the clear takes 17–20 frames, matching tetris.wiki. The rows above drop in one go when it finishes, with no falling animation. A Tetris also turns the background white on each of those frames (`@renderTetrisFlashAndSound`). All three are implemented and pinned by tests in `nes/game.rs`, which replaced the flat 18-frame placeholder.

### Still open

- **OSC palette detection is not implemented** (§7.1). The `SystemAnsi` theme emits symbolic 16-colour codes, which the terminal already renders from the user's own palette, so the visible result is what §7.1 describes as the fallback. Querying OSC 4/10/11 would only add the ability to *choose* slots by measured separation, and it needs raw-mode stdin parsing that fights the event loop for the same bytes — worth doing only if real palettes turn out to make pieces hard to tell apart.

- Exact NES ARE row-boundary cutoffs beyond the documented delta pattern (§3.4).
- NES soft-drop rate (§3.4) — implemented as one row per 2 frames, the commonly cited figure, not confirmed against a disassembly.
- **Spawn headroom.** The ROM's playfield is exactly 10x20 with pieces spawning flat on row 0, but the vertical T/J/L orientations reach a row above their pivot (I reaches two), which would make a piece unrotatable the instant it appears. Two hidden rows were added so rotation works immediately, matching how the real game plays. Worth confirming by playtest against real NES Tetris.
- Guideline combo formula variance across commercial titles (linear `50×combo×level` vs. some titles' lookup tables), and the exact combo-count starting index (§4.5) — implementing the commonly documented linear formula as the reference default.

---

### Critical files once implementation starts
- `src/engine/game_mode.rs` — the trait boundary keeping NES/Modern rules genuinely separate
- `src/engine/nes/randomizer.rs` — highest-risk-of-getting-wrong NES accuracy item
- `src/engine/modern/srs.rs` + `src/engine/modern/tspin.rs` — kick tables + T-spin detection, the trickiest Guideline logic
- `src/input/keymap.rs` — the Action-indirection layer DAS timing and rebinding both depend on
- `src/background/mod.rs` — the `Background` trait + `PerformanceSignal`, which all 11 backgrounds and the reactive cowsay hook build on
- `Cargo.toml` — establishes the core-vs-`audio-visualizer`-feature split that keeps the binary small by default
