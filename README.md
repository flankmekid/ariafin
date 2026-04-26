# ariafin

a terminal music client for jellyfin servers. browse your library, manage playlists, and control playback entirely from the keyboard.

## features

- **home** - recently added albums and recently played tracks
- **artists** - drill down from artist to album to track list
- **albums** - browse all albums; enter to play
- **songs** - full track list with shuffle play
- **playlists** - browse and play server playlists
- **queue** - view and navigate the current play queue
- **search** - live search across artists, albums, and tracks
- **lyrics** - fetch and display synced lyrics for the current track
- **settings** - configure startup tab and default volume (persisted across sessions)
- **playback reporting** - reports play start/stop to jellyfin so your history stays in sync
- **vim-style keybindings** - no mouse needed, just your keyboard (check the keybindings section for more)

## keybindings

| key | action |
|-----|--------|
| `1`-`7` / `Tab`-`Shift+Tab` | switch tabs |
| `j` / `↓` | move down |
| `k` / `↑` | move up |
| `Enter` | select / play |
| `Esc` | go back |
| `Space` | pause / resume |
| `n` | next track |
| `p` | previous track |
| `+` / `-` | volume up / down |
| `r` | toggle repeat |
| `s` | toggle shuffle |
| `l` | fetch lyrics |
| `/` | open search |
| `A` | add / change server |
| `q` | quit |

## installation

ariafin requires [rust](https://rustup.rs/) (stable, 1.75+).

### linux and macos

```sh
git clone https://github.com/flankmekid/ariafin
cd ariafin
cargo install --path crates/af-cli
```

the `ariafin` binary will be placed in `~/.cargo/bin/`. make sure that directory is in your `PATH`.

### windows

```powershell
git clone https://github.com/flankmekid/ariafin
cd ariafin
cargo install --path crates/af-cli
```

the binary is placed in `%USERPROFILE%\.cargo\bin\`. add that to your `PATH` if it is not already there.

### running

```sh
ariafin
```

on first launch, press `A` to add your jellyfin server. enter the server url, username, and password. ariafin will authenticate and sync your library to a local cache.

## configuration

config is stored at:

- linux / macos: `~/.config/ariafin/config.ron`
- windows: `%APPDATA%\ariafin\config.ron`

settings edited in the **Settings** tab are saved automatically.

**Security note:** Credentials (authentication tokens and user IDs) are stored in the system keyring (e.g., Windows Credential Manager, macOS Keychain, Linux Secret Service) rather than in the config file. On first launch after updating, existing credentials will be migrated automatically.

## dependencies

- [ratatui](https://github.com/ratatui-org/ratatui) for the terminal ui
- [cpal](https://github.com/RustAudio/cpal) for audio output
- [symphonia](https://github.com/pdeljanov/Symphonia) for audio decoding
- [tokio](https://tokio.rs/) for async runtime
- [rusqlite](https://github.com/rusqlite/rusqlite) for local library cache

## license

mit
