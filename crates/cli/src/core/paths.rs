use std::path::{Path, PathBuf};

/// Standard layout under `<project_root>/.heal/`.
///
/// All paths are resolved eagerly from a project root so that callers can pass
/// the struct around without re-deriving locations.
#[derive(Debug, Clone)]
pub struct HealPaths {
    root: PathBuf,
}

impl HealPaths {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            root: project_root.as_ref().join(".heal"),
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn config(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// Calibration breaks (`p50` / `p75` / `p90` / `p95` per metric +
    /// per-metric `floor_critical`). Written by `heal init` /
    /// `heal calibrate`; read by every layer that classifies Severity.
    /// Hand-editing is discouraged — only `floor_critical` overrides
    /// belong in `config.toml` instead.
    #[must_use]
    pub fn calibration(&self) -> PathBuf {
        self.root.join("calibration.toml")
    }

    #[must_use]
    pub fn snapshots_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    #[must_use]
    pub fn docs_dir(&self) -> PathBuf {
        self.root.join("docs")
    }

    #[must_use]
    pub fn reports_dir(&self) -> PathBuf {
        self.root.join("reports")
    }

    /// Create every standard subdirectory. Idempotent.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            self.root.as_path(),
            &self.snapshots_dir(),
            &self.logs_dir(),
            &self.docs_dir(),
            &self.reports_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
