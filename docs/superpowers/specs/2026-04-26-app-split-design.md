# Split `af-tui/src/app.rs` into Module Directory

**Date:** 2026-04-26  
**Status:** Approved

## Problem

`crates/af-tui/src/app.rs` is 104 KB / 2547 lines — too large to navigate comfortably.

## Design

Convert the single file into an `app/` module directory with 5 files. Rust allows `impl App` blocks split across files within the same module, so no public API changes are needed.

### File map

| File | Contents | Est. size |
|------|----------|-----------|
| `app/mod.rs` | `App` struct, `new()`, `run()`, `run_loop()`, `server_auth`, `stream_url`, `play_track`, `advance_queue`, `tick`, list-nav helpers (`current_list_len`, `current_selected`, `set_selected`, `list_select`), `trigger_tab_load`, `apply_setting`, `reset_list_for_tab`, `map_key`, `tab_id_to_index`, `TABS`, `SETTING_COUNT` | ~25 KB |
| `app/state.rs` | `QueueState`, `SearchState`, `ArtistView`, `AlbumView`, `PlaylistView` | ~4 KB |
| `app/handlers.rs` | `impl App { handle, handle_enter, handle_search_key, handle_modal_key, handle_bg_event, handle_audio_event }` | ~28 KB |
| `app/background.rs` | `background_worker`, `handle_command`, `load_from_cache`, `perform_sync` | ~7 KB |
| `app/draw.rs` | All `draw_*` functions, `fmt_dur` | ~43 KB |

### Module visibility

- `app/state.rs` types are used only within the `app` module — keep them `pub(super)` (or just `pub` to match existing pattern)
- `background.rs` free functions are `pub(super)` since only `mod.rs` calls them
- `draw.rs` free functions are `pub(super)` since only `mod.rs` calls `draw()`
- `handlers.rs` is a plain `impl App` extension — no visibility change needed

### What does NOT change

- `lib.rs` is unchanged — it still calls `app::run(config)`
- No public types are moved or renamed
- No dependencies change

## Implementation steps

1. Create `crates/af-tui/src/app/` directory
2. Create `app/state.rs` — move `QueueState`, `SearchState`, `ArtistView`, `AlbumView`, `PlaylistView`
3. Create `app/background.rs` — move background/sync free functions
4. Create `app/draw.rs` — move all `draw_*`, `map_key`, `fmt_dur`
5. Create `app/handlers.rs` — move the four `impl App` handler blocks
6. Create `app/mod.rs` from remaining content, adding `mod state; mod handlers; mod background; mod draw;` and `use` statements for moved types
7. Delete `crates/af-tui/src/app.rs`
8. Verify `cargo check` passes
