use anyhow::{Context, Result};
use std::path::PathBuf;
use super::schema::{Config, CURRENT_VERSION};
use crate::secrets;
use serde::Deserialize;

pub fn config_dir() -> Result<PathBuf> {
    dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine platform config directory"))
        .map(|p| p.join("ariafin"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.ron"))
}

/// Load the config file, creating a default one if it does not exist.
pub fn load_or_create() -> Result<Config> {
    let path = config_path()?;

    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        // Try parsing as legacy format first to detect old credentials.
        // If that fails (e.g. new unknown fields), fall back to current format.
        let legacy_result: Result<LegacyConfig, _> = ron::from_str(&raw);

        if let Ok(legacy) = legacy_result {
            let mut migrated_any = false;
            let mut migration_failed = false;

            for server in &legacy.servers {
                if let (Some(token), Some(user_id)) = (&server.token, &server.user_id) {
                    migrated_any = true;
                    if let Err(e) = secrets::store_credentials(&server.base_url, user_id, token) {
                        tracing::error!(
                            "Failed to migrate credentials for {} to keyring: {}",
                            server.name, e
                        );
                        migration_failed = true;
                    } else {
                        tracing::info!(
                            "Migrated credentials for {} to system keyring",
                            server.name
                        );
                    }
                }
            }

            if migrated_any {
                if migration_failed {
                    anyhow::bail!(
                        "Failed to migrate credentials to system keyring. \
                         Please ensure a keyring backend is available and try again."
                    );
                }

                let config = Config {
                    version: CURRENT_VERSION,
                    servers: legacy.servers.into_iter().map(|s| super::schema::ServerConfig {
                        name: s.name,
                        server_type: s.server_type,
                        base_url: s.base_url,
                        username: s.username,
                    }).collect(),
                    active_server: legacy.active_server,
                    ui: legacy.ui,
                    playback: legacy.playback,
                };

                save(&config)?;
                tracing::info!(
                    "Successfully migrated credentials and updated config to version {}",
                    CURRENT_VERSION
                );
                return Ok(config);
            }

            // No old credentials; convert legacy to current config without bumping version.
            let config = Config {
                version: legacy.version,
                servers: legacy.servers.into_iter().map(|s| super::schema::ServerConfig {
                    name: s.name,
                    server_type: s.server_type,
                    base_url: s.base_url,
                    username: s.username,
                }).collect(),
                active_server: legacy.active_server,
                ui: legacy.ui,
                playback: legacy.playback,
            };

            if config.version > CURRENT_VERSION {
                anyhow::bail!(
                    "config version {} is newer than this build supports ({})",
                    config.version,
                    CURRENT_VERSION
                );
            }

            return Ok(config);
        }

        // Legacy parse failed; try current format.
        let config: Config = ron::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;

        if config.version > CURRENT_VERSION {
            anyhow::bail!(
                "config version {} is newer than this build supports ({})",
                config.version,
                CURRENT_VERSION
            );
        }

        Ok(config)
    } else {
        let config = Config::default();
        save(&config).with_context(|| "failed to write default config")?;
        Ok(config)
    }
}

pub fn save(config: &Config) -> Result<()> {
    let path = config_path()?;
    std::fs::create_dir_all(
        path.parent().ok_or_else(|| anyhow::anyhow!("Config path has no parent directory"))?,
    )?;

    let pretty = ron::ser::PrettyConfig::new();
    let content = ron::ser::to_string_pretty(config, pretty)
        .context("failed to serialize config")?;

    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;

    Ok(())
}

// ── Legacy format (pre-keyring) ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LegacyServerConfig {
    pub name: String,
    pub server_type: super::schema::ServerType,
    pub base_url: String,
    pub username: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegacyConfig {
    pub version: u32,
    pub servers: Vec<LegacyServerConfig>,
    pub active_server: Option<String>,
    pub ui: super::schema::UiConfig,
    pub playback: super::schema::PlaybackConfig,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::schema::*;

    fn sample_config() -> Config {
        Config {
            version: CURRENT_VERSION,
            servers: vec![
                ServerConfig {
                    name: "Home".to_string(),
                    server_type: ServerType::Jellyfin,
                    base_url: "http://localhost:8096".to_string(),
                    username: "admin".to_string(),
                },
            ],
            active_server: Some("Home".to_string()),
            ui: UiConfig {
                startup_tab: TabId::Albums,
                layout_density: LayoutDensity::Compact,
                show_lyrics: true,
                show_album_art: true,
                show_visualizer: false,
                visualizer_bars: 32,
            },
            playback: PlaybackConfig {
                default_volume: 50,
                gapless: false,
                crossfade_duration_ms: 2000,
                max_bitrate_kbps: Some(320),
            },
        }
    }

    #[test]
    fn test_config_default_version() {
        let cfg = Config::default();
        assert_eq!(cfg.version, CURRENT_VERSION);
        assert!(cfg.servers.is_empty());
        assert!(cfg.active_server.is_none());
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let cfg = sample_config();
        let pretty = ron::ser::PrettyConfig::new();
        let raw = ron::ser::to_string_pretty(&cfg, pretty).unwrap();
        let decoded: Config = ron::from_str(&raw).unwrap();

        assert_eq!(decoded.version, cfg.version);
        assert_eq!(decoded.servers.len(), cfg.servers.len());
        assert_eq!(decoded.servers[0].name, cfg.servers[0].name);
        assert_eq!(decoded.servers[0].base_url, cfg.servers[0].base_url);
        assert_eq!(decoded.active_server, cfg.active_server);
        assert_eq!(decoded.ui.startup_tab, cfg.ui.startup_tab);
        assert_eq!(decoded.playback.default_volume, cfg.playback.default_volume);
        assert_eq!(decoded.playback.max_bitrate_kbps, cfg.playback.max_bitrate_kbps);
    }

    #[test]
    fn test_version_too_high_fails() {
        let mut cfg = Config::default();
        cfg.version = CURRENT_VERSION + 1;

        let pretty = ron::ser::PrettyConfig::new();
        let raw = ron::ser::to_string_pretty(&cfg, pretty).unwrap();
        let parsed: Config = ron::from_str(&raw).unwrap();

        let result = (|| -> Result<()> {
            if parsed.version > CURRENT_VERSION {
                anyhow::bail!(
                    "config version {} is newer than this build supports ({})",
                    parsed.version,
                    CURRENT_VERSION
                );
            }
            Ok(())
        })();

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("config version"));
        assert!(err.contains(&format!("{}", CURRENT_VERSION + 1)));
    }

    #[test]
    fn test_legacy_format_without_credentials_parses_cleanly() {
        let raw = r#"(
            version: 1,
            servers: [
                (name: "Old", server_type: Jellyfin, base_url: "http://old", username: "u")
            ],
            active_server: Some("Old"),
            ui: (
                startup_tab: Home,
                layout_density: Normal,
                show_lyrics: true,
                show_album_art: false,
                show_visualizer: false,
                visualizer_bars: 20
            ),
            playback: (
                default_volume: 75,
                gapless: true,
                crossfade_duration_ms: 0,
                max_bitrate_kbps: None
            )
        )"#;

        let legacy: Result<LegacyConfig, _> = ron::from_str(raw);
        assert!(legacy.is_ok(), "Legacy config should parse");
        let legacy = legacy.unwrap();
        assert_eq!(legacy.version, 1);
        assert_eq!(legacy.servers[0].name, "Old");
        assert!(legacy.servers[0].token.is_none());
        assert!(legacy.servers[0].user_id.is_none());
    }

    #[test]
    fn test_legacy_format_with_credentials_detected() {
        let raw = r#"(
            version: 1,
            servers: [
                (
                    name: "Old",
                    server_type: Jellyfin,
                    base_url: "http://old",
                    username: "u",
                    token: Some("secret"),
                    user_id: Some("uid")
                )
            ],
            active_server: Some("Old"),
            ui: (
                startup_tab: Home,
                layout_density: Normal,
                show_lyrics: true,
                show_album_art: false,
                show_visualizer: false,
                visualizer_bars: 20
            ),
            playback: (
                default_volume: 75,
                gapless: true,
                crossfade_duration_ms: 0,
                max_bitrate_kbps: None
            )
        )"#;

        let legacy: Result<LegacyConfig, _> = ron::from_str(raw);
        assert!(legacy.is_ok());
        let legacy = legacy.unwrap();
        assert_eq!(legacy.servers[0].token, Some("secret".to_string()));
        assert_eq!(legacy.servers[0].user_id, Some("uid".to_string()));
    }
}
