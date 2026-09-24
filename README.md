# tetris-tui

Rules-accurate Tetris for the terminal, with an NES mode and a modern
Guideline mode. Built with `ratatui` + `crossterm`; no runtime dependencies
beyond a terminal.

The full design, including every researched rule value and its source, is in
[brief.md](brief.md).

## Status

Both modes are playable, with menus, rebindable keys, saved high scores,
selectable visuals and all ten backgrounds. What is left is the Phase 8 polish
pass and the optional audio — see the phase table in `brief.md` §13.

Done so far:

- Shared playfield: collision, line clearing, stack metrics
- **NES ruleset**: the ROM's Nintendo Rotation System tables (no wall kicks), the
  ROM's LFSR piece randomiser (droughts and per-piece bias included), the
  frames-per-row gravity table, DAS charge/repeat with wall charge and
  carry-through-entry-delay, entry delay banding, line-clear delay, scoring and
  level progression
- **Modern (Guideline) ruleset**: SRS with both wall-kick tables, 7-bag with a
  preview queue, hold, ghost piece, hard drop, 500ms lock delay with the 15-reset
  cap, T-spin and mini detection including the fifth-kick promotion, and scoring
  with back-to-back, combos and perfect clears
- Fixed 60 Hz tick loop, rebindable action-based input, Kitty keyboard-protocol
  detection with a timeout fallback
- Board and HUD rendering at two terminal columns per cell, transparency-safe
- Responsive layout that sheds panels as the terminal shrinks
- Title, options, pause and game-over screens, with per-mode top-10 score tables
  and settings saved as you change them
- Three independent visual axes: colour theme, tetromino skin and board border,
  including plain-ASCII and letter-per-cell options for terminals with poor
  Unicode or colour support
- Background layer: blank (keeps terminal transparency), bundled scenes, your
  distribution's logo tiled behind the field, and the animated matrix rain,
  pipes, nyancat, bonsai, aquarium and locomotive, and a cow that comments on
  your play. It runs on the title screen too, as attract mode

## Running

There is no system-wide Rust toolchain on this machine, so use the flake:

```sh
nix develop -c cargo run                  # title screen
nix develop -c cargo run -- nes 9         # straight into NES mode, level 9
nix develop -c cargo run -- modern        # straight into modern mode
nix develop -c cargo run -- modern 5      # modern mode, level 5
```

A mode on the command line skips the title screen and starts a run with it;
anything left out comes from the config file.

Without flakes enabled:

```sh
nix-shell -p cargo rustc gcc --run 'cargo run -- modern'
```

Tests:

```sh
nix develop -c cargo test
```

## Building a release

```sh
nix build                                 # result/bin/tetris-tui
nix run                                   # or run it straight from the flake
nix develop -c cargo build --release      # target/release/tetris-tui
```

Either way it is a single binary of about 1.5 MB with no runtime dependencies.
A flake build only sees files git tracks, so a file added since the last commit
needs a `git add` before `nix build` will find it.

The default build has no audio code or audio dependencies at all. The `audio`
Cargo feature is reserved for the optional music and sound phase (`brief.md`
§12) and does nothing yet.

## Screens

```
Title ──► Playing ⇄ Paused ──► Game over ──► Title
  │          │         │          (name entry on a top-10 run)
  ├─► Options ─────────┘
  └─► High scores
```

Menus are driven by arrow keys or `hjkl`, `Enter` to select and `Esc` to go
back; `q` on the title screen quits, and `Ctrl-C` leaves from anywhere. Those
keys are fixed rather than rebindable, so a keybinding you regret can always be
undone from the menu it was made in.

During a run, `q` abandons it and returns to the title — an abandoned run is not
scored. Topping out is, and a run that makes its mode's top ten asks for a name.

## Options

Everything on the options screen is written to the config file as soon as it
changes:

- **Game mode** — which ruleset `Play` starts
- **Starting level** — remembered separately per mode (NES 0-29, modern 1-20)
- **DAS / ARR / ghost piece** — modern only. NES's equivalents are fixed by its
  ruleset, so they are not offered
- **Colour theme** — `Guideline` is the published piece palette in 24-bit colour;
  `System ANSI` uses the terminal's own 16 colours, which is the one to pick on a
  terminal without truecolor
- **Tetromino skin** — `Solid`, `Shaded`, `Outlined`, `ASCII` (`[]` per cell) or
  `Letters` (the piece's own letter, which identifies pieces without colour)
- **Board border** — `None`, `ASCII`, `Single`, `Double`, `Rounded` or `Heavy`,
  applied to the menus as well as the board
- **Background** — drawn behind the field, never inside the playfield or the HUD
  panels. With either ASCII option picked they keep to ASCII glyphs too.
  - `Blank` — nothing, which keeps terminal transparency
  - `Scene` — a still scene, with a picker, or `Random` for one per session
  - `Distro logo` — your distribution's logo, read from `/etc/os-release`
  - `Matrix rain` — cmatrix's falling columns
  - `Pipes` — pipes.sh, drawn in the board's border style
  - `Nyancat` — an occasional visitor, its rainbow in the theme's colours
  - `Bonsai` — grown by cbonsai's rules in the widest free margin
  - `Aquarium` — asciiquarium's fish, bubbles and seaweed
  - `Cowsay` — reacts to the run: celebrates a Tetris or T-spin clear, cheers a
    combo, gets smug on back-to-back and nervous as the stack nears the top
  - `Locomotive` — sl's steam train every so often, and always one when a run
    tops out
- **Key bindings** — `Enter` on a row, then press the key. A key already bound to
  something else is refused rather than silently stolen

## Files

| Path | Contents |
|---|---|
| `~/.config/tetris-tui/config.toml` | mode, starting levels, DAS/ARR, ghost, theme, skin, border, background, bindings |
| `~/.local/share/tetris-tui/scores.toml` | two top-10 tables, NES and modern kept apart |

Both are plain TOML and meant to be hand-editable, so a mistake costs only
itself: a setting that cannot be read takes its default, and a mangled score row
is dropped, rather than either file being thrown away. Whenever anything had to
be dropped, the file as it was is copied to `config.toml.bak` or
`scores.toml.bak` before the game can write over it. A hand-sorted score table
is re-sorted on load, and both files are written atomically.

## Controls

| Key | Action | |
|---|---|---|
| `←` / `h` | move left | |
| `→` / `l` | move right | |
| `↓` / `j` | soft drop | 1 point per row |
| `Space` | hard drop | modern only, 2 points per row |
| `x` / `↑` | rotate clockwise | |
| `z` | rotate counter-clockwise | |
| `c` / `Tab` | hold | modern only |
| `p` / `Esc` | pause | |
| `q` | quit to title | |

NES mode has no hard drop, no hold and no ghost piece — that is the ruleset, not
a missing feature, and those keys simply do nothing there.

## Input timing

Both rulesets depend on knowing how long a key is *held*, which terminals do not
all report. The HUD shows which mode you are in:

- **precise input** — the terminal supports the Kitty keyboard protocol, so real
  press/release events arrive and DAS is frame-accurate. kitty, WezTerm, foot and
  recent Alacritty qualify.
- **approx. input** — no protocol support, so a release is inferred from a gap in
  autorepeat events. DAS feel is approximate. Plain xterm, VTE-based terminals and
  many tmux/screen setups land here.

This is a terminal limitation rather than something the game can work around.
