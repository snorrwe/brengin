//! Create a loadable tar from the assets directory:
//!
//! ```bash
//! brengin-tools pack-assets assets assets.data
//! ```
use anyhow::Context as _;
use std::{borrow::Cow, io::Read, path::PathBuf};

use crate::asset_registry::{FileLoader, LoadError};

// Store only the file and open the archive for each load (thread) because Archive can't be
// shared between threads. Alternative to consider: put it behind a mutex?
pub struct TarLoader {
    pub archive: PathBuf,
}

impl TarLoader {
    pub fn new(p: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let p = p.into();

        anyhow::ensure!(p.is_file(), "Asset data ({}) not found", p.display());

        Ok(Self { archive: p })
    }

    pub fn assert_checksum(&self, checksum: &str) -> anyhow::Result<()> {
        let digest = sha256::try_digest(self.archive.as_path())
            .context("Failed to compute digest of assets")?;

        anyhow::ensure!(digest == checksum, "Data checksum mismatch");

        Ok(())
    }
}

impl FileLoader for TarLoader {
    fn load_all<'a>(&'a self, p: &std::path::Path) -> Result<Cow<'a, [u8]>, LoadError> {
        let r = std::fs::OpenOptions::new()
            .read(true)
            .open(self.archive.as_path())
            .map_err(|_err| {
                #[cfg(feature = "tracing")]
                tracing::error!(?p,err= ?_err, "Failed to open archive");
                LoadError::NotFound(p.to_path_buf())
            })?;
        let r = std::io::BufReader::new(r);
        let mut archive = tar::Archive::new(r);

        let mut buf = Vec::new();
        for mut e in archive
            .entries_with_seek()
            .map_err(|_err| {
                #[cfg(feature = "tracing")]
                tracing::error!(?p,err= ?_err, "Failed to obtain archive entries");
                LoadError::OpenFailed(p.to_path_buf())
            })?
            .filter_map(|e| e.ok())
        {
            if let Ok(ep) = e.path()
                && ep.trim_prefix("./") == p
                && e.read_to_end(&mut buf).is_ok() {
                    return Ok(Cow::Owned(buf));
                }
        }
        Err(LoadError::NotFound(p.to_path_buf()))
    }
}
