# tetris-tui

Rules-accurate Tetris for the terminal, with an NES mode and a modern
Guideline mode. Built with `ratatui` + `crossterm`; no runtime dependencies
beyond a terminal.

The full design, including every researched rule value and its source, is in
[brief.md](brief.md).

## Status

Both modes are playable, with menus, rebindable keys, saved high scores,
selectable visuals and the first of the backgrounds. The animated backgrounds
(matrix rain, pipes, bonsai, nyancat, aquarium, the reactive cow) are not built
yet — see the phase table in `brief.md` §13.

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
- Background layer, with the still ones built: blank (keeps terminal
  transparency), bundled scenes, and your distribution's logo tiled behind the
  field. It runs on the title screen too, as attract mode

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
- **Background** — `Blank`, `Scene` (with a scene picker, or `Random` for one per
  session) or `Distro logo`, read from `/etc/os-release`. Backgrounds never draw
  inside the playfield or the HUD panels
- **Key bindings** — `Enter` on a row, then press the key. A key already bound to
  something else is refused rather than silently stolen

## Files

| Path | Contents |
|---|---|
| `~/.config/tetris-tui/config.toml` | mode, starting levels, DAS/ARR, ghost, theme, skin, border, background, bindings |
| `~/.local/share/tetris-tui/scores.toml` | two top-10 tables, NES and modern kept apart |

Both are plain TOML and meant to be hand-editable. A missing or corrupt file
falls back to defaults rather than blocking launch, and a hand-sorted score table
is re-sorted on load.

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
