use std::path::{Path, PathBuf};

use crate::config::internal::ActiveConfig;
use crate::error::Error;

#[derive(Debug, Clone)]
pub struct CacheStore {
    pub path: PathBuf,
}

impl CacheStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_last_known_good(&self) -> Result<Option<ActiveConfig>, Error> {
        let _ = self;
        Ok(None)
    }

    pub fn store_validated_snapshot(&self, _config: &ActiveConfig) -> Result<(), Error> {
        let _ = self;
        Ok(())
    }
}
