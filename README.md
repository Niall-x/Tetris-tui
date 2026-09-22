# tetris-tui

Rules-accurate Tetris for the terminal, with an NES mode and a modern
Guideline mode. Built with `ratatui` + `crossterm`; no runtime dependencies
beyond a terminal.

The full design, including every researched rule value and its source, is in
[brief.md](brief.md).

## Status

Both modes are playable. Theming, skins, backgrounds, menus and config
persistence are not built yet — see the phase table in `brief.md` §13.

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

## Running

There is no system-wide Rust toolchain on this machine, so use the flake:

```sh
nix develop -c cargo run                  # NES mode, level 0
nix develop -c cargo run -- nes 9         # NES mode, level 9
nix develop -c cargo run -- modern        # modern mode
nix develop -c cargo run -- modern 5      # modern mode, level 5
```

Without flakes enabled:

```sh
nix-shell -p cargo rustc gcc --run 'cargo run -- modern'
```

Tests:

```sh
nix develop -c cargo test
```

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
| `q` | quit | |

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
