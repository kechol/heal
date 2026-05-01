//! HEAL configuration (`.heal/config.toml`).
//!
//! All structs use `deny_unknown_fields` so typos in user configs surface as
//! schema errors instead of silently dropping settings.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::error::{Error, Result};
use crate::core::fs::atomic_write;

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
    pub policy: PolicyConfig,
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
    pub cognitive: CognitiveConfig,
    #[serde(default = "default_enabled")]
    pub lcom: LcomConfig,
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
            cognitive: CognitiveConfig::enabled(),
            lcom: LcomConfig::enabled(),
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
    #[must_use]
    pub fn top_n_lcom(&self) -> usize {
        self.lcom.top_n.unwrap_or(self.top_n)
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CognitiveConfig {
    pub enabled: bool,
    /// Calibration override — see `core::calibration::FLOOR_COGNITIVE`
    /// for the v0.2 default (50, `SonarQube` Critical baseline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_critical: Option<f64>,
    /// Graduation gate override — see `core::calibration::FLOOR_OK_COGNITIVE`
    /// (8, half of `Sonar`'s "review" threshold). Values strictly below this
    /// classify as Ok regardless of percentile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_ok: Option<f64>,
}

impl Eq for CognitiveConfig {}

impl Toggle for CognitiveConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            floor_critical: None,
            floor_ok: None,
        }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChangeCouplingConfig {
    pub enabled: bool,
    #[serde(default = "default_min_coupling")]
    pub min_coupling: u32,
    /// Lift threshold for filtering coincidental pairs. Lift =
    /// `P(A∩B) / (P(A) × P(B))` — a value of 1.0 means the pair
    /// co-occurs at chance, 2.0 means twice as often as chance. Pairs
    /// below this drop before ranking; default 2.0 keeps strong
    /// associations only.
    #[serde(default = "default_min_lift")]
    pub min_lift: f64,
    /// Threshold both `P(B|A)` and `P(A|B)` must meet for a pair to
    /// classify as `Symmetric` rather than `OneWay`. 0.5 (default) =
    /// each file's edits coincide with the partner at least half the
    /// time. Lower it to surface looser symmetry; raise it to require
    /// near-lockstep changes.
    #[serde(default = "default_symmetric_threshold")]
    pub symmetric_threshold: f64,
    /// Per-metric override for `metrics.top_n` — most-coupled pairs list.
    #[serde(default)]
    pub top_n: Option<usize>,
    /// Calibration override. `min_coupling` already serves as the
    /// scan-time floor, so the absolute Critical floor here is rare in
    /// practice — leave `None` to defer entirely to percentile breaks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_critical: Option<f64>,
}

impl Eq for ChangeCouplingConfig {}

impl Default for ChangeCouplingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_coupling: default_min_coupling(),
            min_lift: default_min_lift(),
            symmetric_threshold: default_symmetric_threshold(),
            top_n: None,
            floor_critical: None,
        }
    }
}

impl Toggle for ChangeCouplingConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            min_coupling: default_min_coupling(),
            min_lift: default_min_lift(),
            symmetric_threshold: default_symmetric_threshold(),
            top_n: None,
            floor_critical: None,
        }
    }
}

fn default_min_coupling() -> u32 {
    3
}

fn default_min_lift() -> f64 {
    2.0
}

fn default_symmetric_threshold() -> f64 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LcomConfig {
    pub enabled: bool,
    /// Extraction backend. v0.2 ships only `tree-sitter-approx`; a
    /// typed `lsp` variant lands in v0.5+. Typo-resistant by virtue
    /// of being an enum.
    #[serde(default)]
    pub backend: LcomBackend,
    /// Classes whose `cluster_count` is below this floor are not
    /// surfaced as Findings. `2` is the natural baseline — `1` means
    /// the class is cohesive and `0` means it has no methods.
    #[serde(default = "default_min_cluster_count")]
    pub min_cluster_count: u32,
    /// Per-metric override for `metrics.top_n` — most-split classes list.
    #[serde(default)]
    pub top_n: Option<usize>,
    /// Calibration override. `min_cluster_count` already serves as the
    /// scan-time floor; absolute Critical floor here is rare.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_critical: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LcomBackend {
    #[default]
    TreeSitterApprox,
    /// Reserved for the v0.5+ LSP-backed implementation; not yet
    /// usable from a `LcomObserver` scan, but the variant is wired so
    /// configs that opt in early don't fail to parse.
    Lsp,
}

impl Eq for LcomConfig {}

impl Default for LcomConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: LcomBackend::default(),
            min_cluster_count: default_min_cluster_count(),
            top_n: None,
            floor_critical: None,
        }
    }
}

impl Toggle for LcomConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            backend: LcomBackend::default(),
            min_cluster_count: default_min_cluster_count(),
            top_n: None,
            floor_critical: None,
        }
    }
}

fn default_min_cluster_count() -> u32 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DuplicationConfig {
    pub enabled: bool,
    #[serde(default = "default_min_tokens")]
    pub min_tokens: u32,
    /// Per-metric override for `metrics.top_n` — largest duplicate blocks.
    #[serde(default)]
    pub top_n: Option<usize>,
    /// Calibration override (per-file duplicate %). v0.2 default is
    /// `core::calibration::FLOOR_DUPLICATION_PCT` (30%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_critical: Option<f64>,
}

impl Eq for DuplicationConfig {}

impl Default for DuplicationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_tokens: default_min_tokens(),
            top_n: None,
            floor_critical: None,
        }
    }
}

impl Toggle for DuplicationConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            min_tokens: default_min_tokens(),
            top_n: None,
            floor_critical: None,
        }
    }
}

fn default_min_tokens() -> u32 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CcnConfig {
    pub enabled: bool,
    #[serde(default = "default_warn_delta_pct")]
    pub warn_delta_pct: u32,
    /// Per-metric override for `metrics.top_n` — covers both CCN and
    /// Cognitive listings since they share the "complexity:" status section.
    #[serde(default)]
    pub top_n: Option<usize>,
    /// Calibration override — see `core::calibration::FLOOR_CCN` for the
    /// v0.2 default (25, `McCabe`'s "untestable" threshold).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_critical: Option<f64>,
    /// Graduation gate override — see `core::calibration::FLOOR_OK_CCN`
    /// (11, `McCabe`'s "simple, low risk" boundary). Values strictly below
    /// this classify as Ok regardless of percentile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_ok: Option<f64>,
}

impl Eq for CcnConfig {}

impl Default for CcnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            warn_delta_pct: default_warn_delta_pct(),
            top_n: None,
            floor_critical: None,
            floor_ok: None,
        }
    }
}

impl Toggle for CcnConfig {
    fn enabled() -> Self {
        Self {
            enabled: true,
            warn_delta_pct: default_warn_delta_pct(),
            top_n: None,
            floor_critical: None,
            floor_ok: None,
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

/// Top-level `[policy]` block. Holds the v0.3 `drain` queue policy plus
/// the reserved-for-future user-defined `rules` map.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    #[serde(default)]
    pub drain: PolicyDrainConfig,
    /// User-defined named policies under `[policy.rules.<name>]`.
    /// Currently parse-only; reserved for v0.4 metric-drift actions.
    #[serde(default)]
    pub rules: BTreeMap<String, PolicyRuleConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PolicyRuleConfig {
    pub action: PolicyAction,
    #[serde(default)]
    pub threshold: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub trigger: Option<String>,
}

/// `[policy.drain]` — which `(severity, hotspot)` combinations the
/// `heal-code-patch` skill must drain (`must`, T0) vs may drain when
/// bandwidth allows (`should`, T1). Anything not matched falls into
/// the Advisory tier (rendered separately, never auto-drained).
///
/// Per-metric overrides under `[policy.drain.metrics.<name>]` let
/// teams tune the drain gate by metric (e.g. stricter `must` for
/// `ccn` because it's a proxy; looser for `duplication` because it's
/// Goodhart-safe).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PolicyDrainConfig {
    /// T0 — drain to zero. Default: `["critical:hotspot"]`.
    #[serde(default = "default_drain_must")]
    pub must: Vec<DrainSpec>,
    /// T1 — drain when convenient. Default:
    /// `["critical", "high:hotspot"]`.
    #[serde(default = "default_drain_should")]
    pub should: Vec<DrainSpec>,
    /// Per-metric overrides keyed by metric name (`ccn`, `cognitive`,
    /// `duplication`, `change_coupling`, `lcom`, `hotspot`). Each
    /// override may set `must` and / or `should`; missing fields fall
    /// back to the global lists above.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metrics: BTreeMap<String, PolicyDrainMetricOverride>,
}

/// One per-metric override under `[policy.drain.metrics.<name>]`.
/// Either field may be `None` to inherit the corresponding global
/// list.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PolicyDrainMetricOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub must: Option<Vec<DrainSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub should: Option<Vec<DrainSpec>>,
}

impl Default for PolicyDrainConfig {
    fn default() -> Self {
        Self {
            must: default_drain_must(),
            should: default_drain_should(),
            metrics: BTreeMap::new(),
        }
    }
}

fn default_drain_must() -> Vec<DrainSpec> {
    vec![DrainSpec {
        severity: crate::core::severity::Severity::Critical,
        hotspot: HotspotMatch::Required,
    }]
}

fn default_drain_should() -> Vec<DrainSpec> {
    vec![
        DrainSpec {
            severity: crate::core::severity::Severity::Critical,
            hotspot: HotspotMatch::Any,
        },
        DrainSpec {
            severity: crate::core::severity::Severity::High,
            hotspot: HotspotMatch::Required,
        },
    ]
}

/// One entry in a `must` / `should` list. The DSL on disk is
/// `<severity>` (any hotspot) or `<severity>:hotspot` (hotspot=true
/// required). Both halves accept lowercase severity names
/// (`critical`, `high`, `medium`, `ok`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainSpec {
    pub severity: crate::core::severity::Severity,
    pub hotspot: HotspotMatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotspotMatch {
    /// Match the spec regardless of the finding's hotspot flag.
    Any,
    /// Match only when the finding has `hotspot = true`.
    Required,
}

impl DrainSpec {
    /// True iff the finding is in scope for this spec (severity and
    /// hotspot constraints both satisfied).
    #[must_use]
    pub fn matches(&self, finding: &crate::core::finding::Finding) -> bool {
        if finding.severity != self.severity {
            return false;
        }
        match self.hotspot {
            HotspotMatch::Any => true,
            HotspotMatch::Required => finding.hotspot,
        }
    }
}

/// Which drain bucket a finding belongs to under a given drain policy.
/// `Must` is the "drain to zero" target; `Should` is the bandwidth-
/// permitting tier; `Advisory` is everything else above `Severity::Ok`.
/// Findings classified as `Severity::Ok` are not surfaced as drain
/// candidates and never reach `Advisory` — see `tier_for`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DrainTier {
    Must,
    Should,
    Advisory,
}

impl PolicyDrainConfig {
    /// Classify a finding into its drain tier. Returns `None` for
    /// `Severity::Ok` findings — those are excluded from drain queues
    /// entirely (the renderer surfaces them via the separate Ok summary).
    /// Per-metric overrides at `[policy.drain.metrics.<name>]` take
    /// precedence over the global `must` / `should` lists; missing
    /// overrides inherit the global value.
    #[must_use]
    pub fn tier_for(&self, finding: &crate::core::finding::Finding) -> Option<DrainTier> {
        if finding.severity == crate::core::severity::Severity::Ok {
            return None;
        }
        let (must, should) = self.specs_for(&finding.metric);
        if must.iter().any(|s| s.matches(finding)) {
            return Some(DrainTier::Must);
        }
        if should.iter().any(|s| s.matches(finding)) {
            return Some(DrainTier::Should);
        }
        Some(DrainTier::Advisory)
    }

    /// Resolve the effective `(must, should)` spec lists for a metric.
    /// Looks up the per-metric override first; metrics without an
    /// override see the global lists. Sub-metrics (`change_coupling.symmetric`)
    /// fall back to their parent (`change_coupling`) before going global.
    fn specs_for(&self, metric: &str) -> (&[DrainSpec], &[DrainSpec]) {
        let mut must: &[DrainSpec] = &self.must;
        let mut should: &[DrainSpec] = &self.should;
        let override_chain =
            std::iter::once(metric).chain(metric.split_once('.').map(|(parent, _)| parent));
        for key in override_chain {
            if let Some(ov) = self.metrics.get(key) {
                if let Some(m) = ov.must.as_ref() {
                    must = m;
                }
                if let Some(s) = ov.should.as_ref() {
                    should = s;
                }
                if ov.must.is_some() && ov.should.is_some() {
                    break;
                }
            }
        }
        (must, should)
    }
}

impl std::str::FromStr for DrainSpec {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut parts = s.split(':');
        let severity_token = parts
            .next()
            .ok_or_else(|| format!("drain spec '{s}' is empty"))?;
        let severity = severity_token
            .parse::<crate::core::severity::Severity>()
            .map_err(|_| {
                format!(
                    "drain spec '{s}' has unknown severity '{severity_token}' (expected one of \
                     critical / high / medium / ok)"
                )
            })?;
        let hotspot = match parts.next() {
            None => HotspotMatch::Any,
            Some("hotspot") => HotspotMatch::Required,
            Some(other) => {
                return Err(format!(
                    "drain spec '{s}' has unknown flag '{other}' (only 'hotspot' is supported)"
                ));
            }
        };
        if parts.next().is_some() {
            return Err(format!(
                "drain spec '{s}' has too many ':' segments (expected at most one)"
            ));
        }
        Ok(Self { severity, hotspot })
    }
}

impl serde::Serialize for DrainSpec {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        let severity = self.severity.as_str();
        let body = match self.hotspot {
            HotspotMatch::Any => severity.to_owned(),
            HotspotMatch::Required => format!("{severity}:hotspot"),
        };
        ser.serialize_str(&body)
    }
}

impl<'de> serde::Deserialize<'de> for DrainSpec {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        s.parse::<Self>().map_err(serde::de::Error::custom)
    }
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

    /// Persist the config atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        let body = self
            .to_toml_string()
            .expect("serialization is infallible for owned data");
        atomic_write(path, body.as_bytes())
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
