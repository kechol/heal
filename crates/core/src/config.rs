//! HEAL configuration (`.heal/config.toml`).
//!
//! All structs use `deny_unknown_fields` so typos in user configs surface as
//! schema errors instead of silently dropping settings.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

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
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub notify: NotifyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub primary_language: Option<String>,
    #[serde(default = "default_docs_dir")]
    pub docs_dir: String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            primary_language: None,
            docs_dir: default_docs_dir(),
        }
    }
}

fn default_docs_dir() -> String {
    ".heal/docs".to_string()
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {
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
    #[serde(default)]
    pub line_coverage: LineCoverageConfig,
    #[serde(default = "default_enabled")]
    pub doc_coverage: ToggleConfig,
    #[serde(default = "default_enabled")]
    pub doc_update_skew: DocUpdateSkewConfig,
    #[serde(default)]
    pub bus_factor: ToggleConfig,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        // Match serde's "section missing" behavior so programmatic `default()`
        // and `from_toml_str("")` produce the same struct.
        Self {
            loc: LocConfig::default(),
            churn: ChurnConfig::enabled(),
            hotspot: HotspotConfig::enabled(),
            change_coupling: ChangeCouplingConfig::enabled(),
            duplication: DuplicationConfig::enabled(),
            ccn: CcnConfig::enabled(),
            cognitive: ToggleConfig::enabled(),
            line_coverage: LineCoverageConfig::default(),
            doc_coverage: ToggleConfig::enabled(),
            doc_update_skew: DocUpdateSkewConfig::enabled(),
            bus_factor: ToggleConfig::default(),
        }
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
}

impl Default for LocConfig {
    fn default() -> Self {
        Self {
            inherit_git_excludes: true,
            exclude_paths: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
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
}

impl Toggle for ChurnConfig {
    fn enabled() -> Self {
        Self { enabled: true }
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
}

impl Eq for HotspotConfig {}

impl Default for HotspotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            weight_churn: default_weight(),
            weight_complexity: default_weight(),
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
}

impl Default for ChangeCouplingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_coupling: default_min_coupling(),
        }
    }
}

impl Toggle for ChangeCouplingConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            min_coupling: default_min_coupling(),
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
}

impl Default for DuplicationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_tokens: default_min_tokens(),
        }
    }
}

impl Toggle for DuplicationConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            min_tokens: default_min_tokens(),
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
}

impl Default for CcnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            warn_delta_pct: default_warn_delta_pct(),
        }
    }
}

impl Toggle for CcnConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            warn_delta_pct: default_warn_delta_pct(),
        }
    }
}

fn default_warn_delta_pct() -> u32 {
    30
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LineCoverageConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub lcov_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocUpdateSkewConfig {
    pub enabled: bool,
    #[serde(default = "default_max_skew_days")]
    pub max_skew_days: u32,
}

impl Default for DocUpdateSkewConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_skew_days: default_max_skew_days(),
        }
    }
}

impl Toggle for DocUpdateSkewConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            max_skew_days: default_max_skew_days(),
        }
    }
}

fn default_max_skew_days() -> u32 {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    #[serde(default = "default_agent_provider")]
    pub provider: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: default_agent_provider(),
        }
    }
}

fn default_agent_provider() -> String {
    "claude-code".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NotifyConfig {
    #[serde(default = "default_stop_hook_message")]
    pub stop_hook_message: String,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            stop_hook_message: default_stop_hook_message(),
        }
    }
}

fn default_stop_hook_message() -> String {
    "HEAL: コードのメンテナンスが必要なタイミングです。`heal status` で確認できます".to_string()
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

    pub fn from_toml_str(s: &str) -> std::result::Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

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

    /// Default config for a new project. The caller is expected to fine-tune
    /// based on `heal init` auto-detection (solo vs team etc.) afterwards.
    #[must_use]
    pub fn recommended_for_solo() -> Self {
        let mut cfg = Self {
            project: ProjectConfig::default(),
            git: GitConfig::default(),
            metrics: MetricsConfig::default(),
            policy: BTreeMap::new(),
            agent: AgentConfig::default(),
            notify: NotifyConfig::default(),
        };
        cfg.metrics.bus_factor.enabled = false;
        cfg.policy.insert(
            "high_complexity_new_function".to_string(),
            PolicyConfig {
                action: PolicyAction::ReportOnly,
                threshold: BTreeMap::from([
                    ("ccn".to_string(), toml::Value::Integer(15)),
                    ("delta_pct".to_string(), toml::Value::Integer(20)),
                ]),
                trigger: None,
                cooldown_hours: 24,
            },
        );
        cfg
    }
}

/// Convenience: load from `.heal/config.toml` under a project root.
pub fn load_from_project(project_root: &Path) -> Result<Config> {
    Config::load(&crate::paths::HealPaths::new(project_root).config())
}
