# Agent Documentation for `ariafin`

## Overview
`ariafin` is a terminal music player for Jellyfin & Navidrome, built with Rust. It consists of several crates organized in a workspace:
- `af-core`: Core functionality (config, cache, types, events).
- `af-api`: API interactions with music servers.
- `af-tui`: Terminal user interface.
- `af-cli`: Command-line interface.
- `af-daemon`: Background daemon for playback.
- `af-audio`: Audio playback and streaming.

## Essential Commands
- **Build**: `cargo build` (or `cargo build --release` for optimized builds).
- **Run**: `cargo run --bin ariafin` (or `./target/release/ariafin`).
- **Test**: `cargo test` (runs all tests in the workspace).
- **Lint**: `cargo clippy` (ensure Rust best practices).

## Code Organization
- **Config**: Managed in `af-core/config` (RON format).
- **API**: Implemented in `af-api` (Jellyfin/Navidrome support).
- **TUI**: Built with `ratatui` and `crossterm` in `af-tui`.
- **Audio**: Uses `symphonia` and `cpal` in `af-audio`.

## Patterns & Conventions
- **Error Handling**: Uses `anyhow` and `thiserror` for consistent error propagation.
- **Logging**: `tracing` for structured logging (logs to `~/.local/share/ariafin/ariafin.log`).
- **Async**: `tokio` for async runtime; `async-trait` for trait implementations.

## Gotchas
- **Config Path**: Defaults to `~/.config/ariafin/config.ron`; can be overridden with `--config`.
- **Logging**: Avoid `tracing` in the TUI to prevent terminal corruption.
- **Dependencies**: Workspace-wide dependencies are defined in `Cargo.toml`; crates inherit them.

## Testing
- Unit tests are co-located with source files.
- Integration tests (if any) should be in `tests/` directories.

## Architecture
- **Control Flow**: CLI → TUI → API/Audio → Core.
- **Data Flow**: Config → Core → API/TUI/Audio.

## Notes for Agents
- Focus on `af-core` for foundational changes.
- `af-tui` is sensitive to terminal state; avoid direct stdout/stderr writes.
- Audio playback (`af-audio`) is async; ensure proper `tokio` runtime handling.