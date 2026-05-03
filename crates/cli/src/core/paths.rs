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

    /// Append-only result cache: `<root>/checks/YYYY-MM.jsonl` plus
    /// auxiliary files (`latest.json`, `fixed.jsonl`, `regressed.jsonl`).
    /// Skill workflows read this; `heal status` writes it. Kept under
    /// `.heal/` (not `.cache/`) so it ships with the project alongside
    /// snapshots.
    #[must_use]
    pub fn checks_dir(&self) -> PathBuf {
        self.root.join("checks")
    }

    /// Latest `CheckRecord` mirror, written atomically after every
    /// `heal status`. Skills that just want "the current TODO list" read
    /// this without scanning the JSONL stream.
    #[must_use]
    pub fn checks_latest(&self) -> PathBuf {
        self.checks_dir().join("latest.json")
    }

    /// "These findings have been fixed by a commit" log. Append-only.
    /// Reconciled on every `heal status` run — entries that re-detect
    /// are removed and surfaced in `regressed.jsonl`.
    #[must_use]
    pub fn checks_fixed_log(&self) -> PathBuf {
        self.checks_dir().join("fixed.jsonl")
    }

    /// Regression audit trail. Append-only. Each entry ties a fixed
    /// finding to the `check_id` that re-detected it.
    #[must_use]
    pub fn checks_regressed_log(&self) -> PathBuf {
        self.checks_dir().join("regressed.jsonl")
    }

    /// Manifest tracking which skill files were extracted by
    /// `heal skills install`, keyed by `<skill-name>/<rel-path>`. Lives
    /// under `.heal/` (not `.claude/skills/`) so the manifest stays
    /// heal-owned and Claude never reads files it doesn't recognise.
    #[must_use]
    pub fn skills_install_manifest(&self) -> PathBuf {
        self.root.join("skills-install.json")
    }

    /// Create every standard subdirectory. Idempotent.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [
            self.root.as_path(),
            &self.snapshots_dir(),
            &self.checks_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
