use anyhow::{Context, Result};
use std::path::PathBuf;
use super::schema::{Config, CURRENT_VERSION};

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .expect("could not determine platform config directory")
        .join("ariafin")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.ron")
}

/// Load the config file, creating a default one if it does not exist.
pub fn load_or_create() -> Result<Config> {
    let path = config_path();

    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
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
    let path = config_path();
    std::fs::create_dir_all(
        path.parent().expect("config path has no parent"),
    )?;

    let pretty = ron::ser::PrettyConfig::new();
    let content = ron::ser::to_string_pretty(config, pretty)
        .context("failed to serialize config")?;

    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;

    Ok(())
}
