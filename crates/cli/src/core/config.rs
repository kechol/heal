//! HEAL configuration (`.heal/config.toml`).
//!
//! All structs use `deny_unknown_fields` so typos in user configs surface as
//! schema errors instead of silently dropping settings.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub project: ProjectConfig,
    #[serde(default)]
    pub git: GitConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub policy: BTreeMap<String, PolicyConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// Natural language for AI-generated explanations (`heal check`
    /// output, future `run-*` proposals). Free-form so users can write
    /// `"Japanese"`, `"日本語"`, `"ja"`, `"français"` — the value is
    /// passed verbatim to the model. `None` keeps the model default.
    #[serde(default)]
    pub response_language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GitConfig {
    #[serde(default = "default_since_days")]
    pub since_days: u32,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            since_days: default_since_days(),
            exclude_paths: Vec::new(),
        }
    }
}

fn default_since_days() -> u32 {
    90
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {
    /// Default `worst_n` width for status rankings. Each metric below can
    /// override this with its own `top_n = N`; absent overrides fall back
    /// to this value.
    #[serde(default = "default_top_n")]
    pub top_n: usize,
    #[serde(default)]
    pub loc: LocConfig,
    #[serde(default = "default_enabled")]
    pub churn: ChurnConfig,
    #[serde(default = "default_enabled")]
    pub hotspot: HotspotConfig,
    #[serde(default = "default_enabled")]
    pub change_coupling: ChangeCouplingConfig,
    #[serde(default = "default_enabled")]
    pub duplication: DuplicationConfig,
    #[serde(default = "default_enabled")]
    pub ccn: CcnConfig,
    #[serde(default = "default_enabled")]
    pub cognitive: ToggleConfig,
}

impl Eq for MetricsConfig {}

impl Default for MetricsConfig {
    fn default() -> Self {
        // Match serde's "section missing" behavior so programmatic `default()`
        // and `from_toml_str("")` produce the same struct.
        Self {
            top_n: default_top_n(),
            loc: LocConfig::default(),
            churn: ChurnConfig::enabled(),
            hotspot: HotspotConfig::enabled(),
            change_coupling: ChangeCouplingConfig::enabled(),
            duplication: DuplicationConfig::enabled(),
            ccn: CcnConfig::enabled(),
            cognitive: ToggleConfig::enabled(),
        }
    }
}

impl MetricsConfig {
    /// Resolve the effective `top_n` for a given metric: per-metric override
    /// wins, otherwise fall back to the global `metrics.top_n`.
    #[must_use]
    pub fn top_n_loc(&self) -> usize {
        self.loc.top_n.unwrap_or(self.top_n)
    }
    #[must_use]
    pub fn top_n_complexity(&self) -> usize {
        self.ccn.top_n.unwrap_or(self.top_n)
    }
    #[must_use]
    pub fn top_n_churn(&self) -> usize {
        self.churn.top_n.unwrap_or(self.top_n)
    }
    #[must_use]
    pub fn top_n_change_coupling(&self) -> usize {
        self.change_coupling.top_n.unwrap_or(self.top_n)
    }
    #[must_use]
    pub fn top_n_duplication(&self) -> usize {
        self.duplication.top_n.unwrap_or(self.top_n)
    }
    #[must_use]
    pub fn top_n_hotspot(&self) -> usize {
        self.hotspot.top_n.unwrap_or(self.top_n)
    }
}

/// LOC has no enable/disable toggle: it is a foundational metric that other
/// observers (hotspot, churn weighting, primary-language detection) depend on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocConfig {
    #[serde(default = "default_true")]
    pub inherit_git_excludes: bool,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    /// Per-metric override for `metrics.top_n` — only the top languages list.
    #[serde(default)]
    pub top_n: Option<usize>,
}

impl Default for LocConfig {
    fn default() -> Self {
        Self {
            inherit_git_excludes: true,
            exclude_paths: Vec::new(),
            top_n: None,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_top_n() -> usize {
    5
}

trait Toggle {
    fn enabled() -> Self;
}

fn default_enabled<T: Toggle>() -> T {
    T::enabled()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToggleConfig {
    pub enabled: bool,
}

impl Toggle for ToggleConfig {
    fn enabled() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChurnConfig {
    pub enabled: bool,
    /// Per-metric override for `metrics.top_n` — most-churned files list.
    #[serde(default)]
    pub top_n: Option<usize>,
}

impl Toggle for ChurnConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            top_n: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HotspotConfig {
    pub enabled: bool,
    #[serde(default = "default_weight")]
    pub weight_churn: f64,
    #[serde(default = "default_weight")]
    pub weight_complexity: f64,
    /// Per-metric override for `metrics.top_n` — top hotspot files. Also
    /// drives the new-in-top-N membership diff in `SnapshotDelta`.
    #[serde(default)]
    pub top_n: Option<usize>,
}

impl Eq for HotspotConfig {}

impl Default for HotspotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            weight_churn: default_weight(),
            weight_complexity: default_weight(),
            top_n: None,
        }
    }
}

impl Toggle for HotspotConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

fn default_weight() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChangeCouplingConfig {
    pub enabled: bool,
    #[serde(default = "default_min_coupling")]
    pub min_coupling: u32,
    /// Per-metric override for `metrics.top_n` — most-coupled pairs list.
    #[serde(default)]
    pub top_n: Option<usize>,
}

impl Default for ChangeCouplingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_coupling: default_min_coupling(),
            top_n: None,
        }
    }
}

impl Toggle for ChangeCouplingConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            min_coupling: default_min_coupling(),
            top_n: None,
        }
    }
}

fn default_min_coupling() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DuplicationConfig {
    pub enabled: bool,
    #[serde(default = "default_min_tokens")]
    pub min_tokens: u32,
    /// Per-metric override for `metrics.top_n` — largest duplicate blocks.
    #[serde(default)]
    pub top_n: Option<usize>,
}

impl Default for DuplicationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_tokens: default_min_tokens(),
            top_n: None,
        }
    }
}

impl Toggle for DuplicationConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            min_tokens: default_min_tokens(),
            top_n: None,
        }
    }
}

fn default_min_tokens() -> u32 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CcnConfig {
    pub enabled: bool,
    #[serde(default = "default_warn_delta_pct")]
    pub warn_delta_pct: u32,
    /// Per-metric override for `metrics.top_n` — covers both CCN and
    /// Cognitive listings since they share the "complexity:" status section.
    #[serde(default)]
    pub top_n: Option<usize>,
}

impl Default for CcnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            warn_delta_pct: default_warn_delta_pct(),
            top_n: None,
        }
    }
}

impl Toggle for CcnConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            warn_delta_pct: default_warn_delta_pct(),
            top_n: None,
        }
    }
}

fn default_warn_delta_pct() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub enum PolicyAction {
    ReportOnly,
    Notify,
    Propose,
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    pub action: PolicyAction,
    #[serde(default)]
    pub threshold: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default = "default_cooldown_hours")]
    pub cooldown_hours: u32,
}

fn default_cooldown_hours() -> u32 {
    24
}

impl Config {
    /// Read and validate a config from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::ConfigMissing(path.to_path_buf())
            } else {
                Error::Io {
                    path: path.to_path_buf(),
                    source: e,
                }
            }
        })?;
        Self::from_toml_str(&raw).map_err(|source| Error::ConfigParse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Parse a TOML body. Useful for tests and for round-trip checks.
    #[must_use = "ignoring the parse result will silently swallow schema errors"]
    pub fn from_toml_str(s: &str) -> std::result::Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Serialize back to TOML. The struct is owned so this is infallible
    /// for any value produced by `Default::default()` or `Config::load`.
    #[must_use = "the serialised string is the only return value"]
    pub fn to_toml_string(&self) -> std::result::Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Persist the config (creates parent dirs).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let body = self
            .to_toml_string()
            .expect("serialization is infallible for owned data");
        std::fs::write(path, body).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })
    }

    /// The exclude-path list every observer should honor: `git.exclude_paths`
    /// (when `metrics.loc.inherit_git_excludes` is true) followed by
    /// `metrics.loc.exclude_paths`. The LOC config holds the canonical
    /// project-wide exclude set so a single `[metrics.loc]` edit propagates.
    #[must_use]
    pub fn observer_excluded_paths(&self) -> Vec<String> {
        let mut excluded: Vec<String> = if self.metrics.loc.inherit_git_excludes {
            self.git.exclude_paths.clone()
        } else {
            Vec::new()
        };
        excluded.extend(self.metrics.loc.exclude_paths.iter().cloned());
        excluded
    }
}

/// Convenience: load from `.heal/config.toml` under a project root.
pub fn load_from_project(project_root: &Path) -> Result<Config> {
    Config::load(&crate::core::paths::HealPaths::new(project_root).config())
}
