mod db;
pub use db::CacheDb;

use std::path::PathBuf;
use anyhow::Result;

pub fn cache_db_path() -> Result<PathBuf> {
    Ok(dirs::cache_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine platform cache directory"))?
        .join("ariafin")
        .join("library.db"))
}
