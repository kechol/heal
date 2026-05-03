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
    /// `fixed.json`, `regressed.jsonl`, `accepted.json`. `heal status`
    /// writes it; skill workflows and `heal diff` read it. Tracked by
    /// git alongside `config.toml` and `calibration.toml` so teammates
    /// on the same commit see identical drain queues without
    /// re-scanning.
    #[must_use]
    pub fn findings_dir(&self) -> PathBuf {
        self.root.join("findings")
    }

    /// Latest `FindingsRecord` mirror, atomically written after every
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
    /// finding to the `id` of the `FindingsRecord` that re-detected it.
    #[must_use]
    pub fn findings_regressed_log(&self) -> PathBuf {
        self.findings_dir().join("regressed.jsonl")
    }

    /// "These findings are accepted as intrinsic / won't fix" map,
    /// keyed by `Finding.id`. Decorates `Finding.accepted: bool` at
    /// render time so accepted findings are excluded from the drain
    /// queue and the Population counts. Tracked alongside
    /// `fixed.json` so the team's acceptance decisions survive across
    /// machines.
    #[must_use]
    pub fn findings_accepted(&self) -> PathBuf {
        self.findings_dir().join("accepted.json")
    }

    /// `.heal/.gitignore` — written by `heal init`. Currently empty
    /// (the findings cache is tracked); reserved for future
    /// per-machine carve-outs.
    #[must_use]
    pub fn gitignore(&self) -> PathBuf {
        self.root.join(".gitignore")
    }

    /// Create every standard subdirectory. Idempotent.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.root.as_path(), &self.findings_dir()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
