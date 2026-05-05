use std::path::{Path, PathBuf};

/// Walk up from `start` looking for an ancestor containing a
/// `.heal/config.toml` and return that ancestor as the project root.
/// Falls back to `start` when no ancestor qualifies, which preserves
/// the "current directory" default for `heal init` on a fresh project
/// while letting `heal status` / `heal diff` work from any
/// subdirectory of an already-initialized repo.
///
/// The marker is `.heal/config.toml` rather than `.heal/` itself
/// because `heal status` calls `HealPaths::ensure()` before any
/// config load — every error-path invocation from a subdirectory
/// would otherwise leave behind an empty `.heal/` that the next
/// walk-up would mistakenly stop at.
#[must_use]
pub fn find_project_root(start: &Path) -> PathBuf {
    for ancestor in start.ancestors() {
        if HealPaths::new(ancestor).config().is_file() {
            return ancestor.to_path_buf();
        }
    }
    start.to_path_buf()
}

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

    /// Create every standard subdirectory. Idempotent.
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [self.root.as_path(), &self.findings_dir()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
