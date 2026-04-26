mod app;
mod input;
mod state;
mod theme;
mod widgets;

use af_core::config::Config;
use anyhow::Result;

pub async fn run(config: Config) -> Result<()> {
    app::run(config).await
}
