mod db;
pub use db::CacheDb;

use std::path::PathBuf;

pub fn cache_db_path() -> PathBuf {
    dirs::cache_dir()
        .expect("could not determine platform cache directory")
        .join("ariafin")
        .join("library.db")
}
