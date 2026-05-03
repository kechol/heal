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

    /// Single-record finding cache: `<root>/findings/latest.json`,
    /// `fixed.json`, `regressed.jsonl`. `heal status` writes it; skill
    /// workflows and `heal diff` read it. Kept under `.heal/` (not
    /// `.cache/`) so the layout ships with the project alongside
    /// `config.toml` and `calibration.toml`. Untracked by git via the
    /// `.heal/.gitignore` `heal init` writes.
    #[must_use]
    pub fn findings_dir(&self) -> PathBuf {
        self.root.join("findings")
    }

    /// Latest `CheckRecord` mirror, atomically written after every
    /// `heal status`. The single source of truth for "what does the
    /// project look like right now".
    #[must_use]
    pub fn findings_latest(&self) -> PathBuf {
        self.findings_dir().join("latest.json")
    }

    /// "These findings have been fixed by a commit" map, keyed by
    /// `Finding.id`. Bounded by the number of currently-tracked fixes
    /// (no append-only growth). Reconciled on every `heal status` run —
    /// entries whose finding re-detects are removed from the map and
    /// surfaced in `regressed.jsonl`.
    #[must_use]
    pub fn findings_fixed(&self) -> PathBuf {
        self.findings_dir().join("fixed.json")
    }

    /// Regression audit trail. Append-only. Each entry ties a fixed
    /// finding to the `check_id` that re-detected it.
    #[must_use]
    pub fn findings_regressed_log(&self) -> PathBuf {
        self.findings_dir().join("regressed.jsonl")
    }

    /// `.heal/.gitignore` — written by `heal init` so volatile state
    /// (`findings/`, `skills-install.json`) doesn't dirty the worktree.
    /// Tracked tomls (`config.toml`, `calibration.toml`) stay versioned.
    #[must_use]
    pub fn gitignore(&self) -> PathBuf {
        self.root.join(".gitignore")
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
        for dir in [self.root.as_path(), &self.findings_dir()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
