//! Severity ladder used by Calibration.
//!
//! `Severity` is intentionally a four-step ladder, ordered `Ok < Medium
//! < High < Critical` so per-file aggregation can use `cmp::max` (TODO
//! §「1 ファイルに複数メトリクスがある場合の Severity は最大値採用」).
//! The default is `Ok` so a `Finding` with no calibration applied
//! reads as "uncalibrated / acceptable".
//!
//! Classification proper lives in [`crate::core::calibration`]; this
//! module owns only the type itself so observers can import the
//! enum without pulling in the full calibration loader.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default, Hash,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Ok,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// Lowercase string label, matching the serde representation. Used
    /// where `serde_json` would be overkill — DSL parsing, log lines,
    /// CLI output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

impl FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "ok" => Ok(Self::Ok),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => Err(format!(
                "unknown severity '{other}' (expected one of critical / high / medium / ok)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ord_runs_ok_to_critical() {
        assert!(Severity::Ok < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
    }

    #[test]
    fn max_picks_highest() {
        assert_eq!(
            Severity::Critical,
            std::cmp::max(Severity::Ok, Severity::Critical)
        );
        assert_eq!(
            Severity::High,
            std::cmp::max(Severity::Medium, Severity::High)
        );
    }

    #[test]
    fn default_is_ok() {
        assert_eq!(Severity::default(), Severity::Ok);
    }

    #[test]
    fn serialises_lowercase() {
        let json = serde_json::to_string(&Severity::Critical).unwrap();
        assert_eq!(json, "\"critical\"");
        let back: Severity = serde_json::from_str("\"medium\"").unwrap();
        assert_eq!(back, Severity::Medium);
    }

    #[test]
    fn from_str_parses_canonical_lowercase() {
        assert_eq!("critical".parse::<Severity>().unwrap(), Severity::Critical);
        assert_eq!("high".parse::<Severity>().unwrap(), Severity::High);
        assert_eq!("medium".parse::<Severity>().unwrap(), Severity::Medium);
        assert_eq!("ok".parse::<Severity>().unwrap(), Severity::Ok);
        assert!("blocker".parse::<Severity>().is_err());
    }

    #[test]
    fn as_str_round_trips_through_from_str() {
        for s in [
            Severity::Ok,
            Severity::Medium,
            Severity::High,
            Severity::Critical,
        ] {
            assert_eq!(s.as_str().parse::<Severity>().unwrap(), s);
        }
    }
}

/// Tally of Findings by Severity. Carried inside `FindingsRecord` and the
/// post-commit nudge — the canonical "how dirty is the codebase" counts.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeverityCounts {
    #[serde(default)]
    pub critical: u32,
    #[serde(default)]
    pub high: u32,
    #[serde(default)]
    pub medium: u32,
    #[serde(default)]
    pub ok: u32,
}

impl SeverityCounts {
    /// Tally one classification result. Saturating-add so a 4-billion
    /// finding codebase doesn't wrap to 0 (it would have other
    /// problems by then).
    pub fn tally(&mut self, severity: Severity) {
        let bucket = match severity {
            Severity::Critical => &mut self.critical,
            Severity::High => &mut self.high,
            Severity::Medium => &mut self.medium,
            Severity::Ok => &mut self.ok,
        };
        *bucket = bucket.saturating_add(1);
    }

    /// Build a `SeverityCounts` from a slice of findings.
    #[must_use]
    pub fn from_findings(findings: &[crate::core::finding::Finding]) -> Self {
        let mut counts = Self::default();
        for f in findings {
            counts.tally(f.severity);
        }
        counts
    }

    /// Inline summary line for human-facing CLI output, e.g.
    /// `[critical] 3   [high] 12   [medium] 28   [ok] 412`. When
    /// `colorize` is true the four labels carry ANSI SGR codes (red /
    /// yellow / cyan / green) suitable for a terminal; pass `false`
    /// when piping to a file.
    #[must_use]
    pub fn render_inline(&self, colorize: bool) -> String {
        use crate::core::term::{ansi_wrap, ANSI_CYAN, ANSI_GREEN, ANSI_RED, ANSI_YELLOW};
        format!(
            "{} {}   {} {}   {} {}   {} {}",
            ansi_wrap(ANSI_RED, "[critical]", colorize),
            self.critical,
            ansi_wrap(ANSI_YELLOW, "[high]", colorize),
            self.high,
            ansi_wrap(ANSI_CYAN, "[medium]", colorize),
            self.medium,
            ansi_wrap(ANSI_GREEN, "[ok]", colorize),
            self.ok,
        )
    }
}

#[cfg(test)]
mod severity_counts_tests {
    use super::*;

    #[test]
    fn render_inline_plain_has_no_ansi() {
        let c = SeverityCounts {
            critical: 3,
            high: 12,
            medium: 28,
            ok: 412,
        };
        let s = c.render_inline(false);
        assert!(
            !s.contains('\x1b'),
            "plain render must not include ANSI codes"
        );
        assert!(s.contains("[critical] 3"));
        assert!(s.contains("[high] 12"));
        assert!(s.contains("[medium] 28"));
        assert!(s.contains("[ok] 412"));
    }

    #[test]
    fn render_inline_colored_has_reset_after_each_label() {
        let c = SeverityCounts::default();
        let s = c.render_inline(true);
        assert_eq!(s.matches("\x1b[0m").count(), 4);
    }
}
