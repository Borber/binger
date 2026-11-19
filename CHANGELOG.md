# Changelog

## [Unreleased]

## [0.1.1] - 2026-11-19

### Added

- Automatic XDG configuration lookup at `$XDG_CONFIG_HOME/binger/config.toml` or `~/.config/binger/config.toml`.
- Explicit `--config PATH` override for selecting a different configuration file.

## [0.1.0] - 2026-11-19

### Added

- Wayland overlay for keyboard-driven area selection.
- `asdfqwer` area selection with `qwer` on the top row and `asdf` on the bottom row.
- `u`, `i`, and `o` for left, right, and middle click.
- `hjkl` pointer refinement with accelerated key repeat.
- System font rendering with scale-aware labels.
- Hyprland focus recovery after another layer surface closes.
- Configurable key maps and overlay settings.
