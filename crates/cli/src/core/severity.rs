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
}
