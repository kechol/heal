//! lcov.info reader. lcov is a flat per-record format the major
//! coverage reporters all converge on:
//!
//! - `cargo llvm-cov` → `target/llvm-cov/lcov.info`
//! - `pytest --cov` → `coverage/lcov.info` (via `coverage.lcov`)
//! - `nyc` (Istanbul) → `coverage/lcov.info` or `lcov-report/lcov.info`
//! - `scoverage` → `target/scoverage-report/scoverage.xml` + lcov export
//!
//! Format (one record per source file, terminated by `end_of_record`):
//! ```text
//! SF:<path>
//! DA:<line>,<hits>            # one per executable line
//! LF:<count>                  # total instrumented lines
//! LH:<count>                  # lines hit
//! BRDA:<line>,<block>,<branch>,<taken>
//! BRF:<count>                 # total branches
//! BRH:<count>                 # branches hit
//! end_of_record
//! ```
//!
//! Reporter dialects diverge on which records are emitted (some skip
//! per-line `DA` and only summarise via `LF`/`LH`; some omit `BRDA`).
//! The reader is permissive: unknown record kinds are ignored, and
//! a missing `LF`/`LH` is recovered from the `DA` lines when present.

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::error::{Error, Result};

/// Coverage summary for one source file. `path` is rendered exactly as
/// the lcov record carried it (some reporters write absolute paths,
/// some project-relative); the caller is responsible for normalising
/// against the project root via [`normalise_lcov_path`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LcovFile {
    pub path: PathBuf,
    pub lines_found: u32,
    pub lines_hit: u32,
    pub branches_found: u32,
    pub branches_hit: u32,
}

impl LcovFile {
    /// Line coverage as a 0.0–1.0 ratio. Files with no instrumented
    /// lines (`lines_found == 0`) return `1.0` — by convention, a
    /// file with no executable lines is fully covered (it carries no
    /// risk, so penalising it would create dead-coverage signal).
    #[must_use]
    pub fn line_coverage_ratio(&self) -> f64 {
        if self.lines_found == 0 {
            return 1.0;
        }
        f64::from(self.lines_hit) / f64::from(self.lines_found)
    }

    /// Coverage as a 0.0–100.0 percentage; the form fed into
    /// `[calibration.coverage_pct]` after inversion.
    #[must_use]
    pub fn line_coverage_pct(&self) -> f64 {
        self.line_coverage_ratio() * 100.0
    }
}

/// Parsed lcov file as a flat list of per-source records. Order
/// matches the file order; duplicates (same `SF`) are merged additively
/// because some reporters emit one record per test entry-point and
/// expect the consumer to sum them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LcovReport {
    pub files: Vec<LcovFile>,
}

impl LcovReport {
    /// Read and parse `path`. Missing files are surfaced as
    /// [`Error::Io`] — callers (the Coverage observer's path-probe
    /// loop) treat absence as "no record" and continue.
    pub fn read(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(Self::parse(&raw))
    }

    /// Parse a raw lcov body. Permissive: unknown record kinds and
    /// malformed numeric fields are skipped silently. The only fatal
    /// case is "no `end_of_record` ever, no `SF` ever" — which yields
    /// an empty report, also non-fatal.
    #[must_use]
    pub fn parse(body: &str) -> Self {
        let mut files: Vec<LcovFile> = Vec::new();
        let mut cur: Option<LcovRecord> = None;
        for raw_line in body.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "end_of_record" {
                if let Some(rec) = cur.take() {
                    files.push(rec.finalise());
                }
                continue;
            }
            let Some((kind, value)) = line.split_once(':') else {
                continue;
            };
            // Unknown record kinds (e.g. `TN:<test_name>` test-name
            // headers, `VER:`, reporter-private extensions) fall through
            // the `_ => {}` arm and are silently ignored.
            match kind {
                "SF" => {
                    if let Some(rec) = cur.take() {
                        files.push(rec.finalise());
                    }
                    cur = Some(LcovRecord::new(PathBuf::from(value)));
                }
                "DA" => {
                    if let Some(rec) = cur.as_mut() {
                        rec.note_da(value);
                    }
                }
                "LF" => {
                    if let Some(rec) = cur.as_mut() {
                        if let Ok(n) = value.parse::<u32>() {
                            rec.lf = Some(n);
                        }
                    }
                }
                "LH" => {
                    if let Some(rec) = cur.as_mut() {
                        if let Ok(n) = value.parse::<u32>() {
                            rec.lh = Some(n);
                        }
                    }
                }
                "BRF" => {
                    if let Some(rec) = cur.as_mut() {
                        if let Ok(n) = value.parse::<u32>() {
                            rec.brf = Some(n);
                        }
                    }
                }
                "BRH" => {
                    if let Some(rec) = cur.as_mut() {
                        if let Ok(n) = value.parse::<u32>() {
                            rec.brh = Some(n);
                        }
                    }
                }
                "BRDA" => {
                    if let Some(rec) = cur.as_mut() {
                        rec.note_brda(value);
                    }
                }
                _ => {}
            }
        }
        // Some reporters omit a final `end_of_record`. Don't drop the tail.
        if let Some(rec) = cur.take() {
            files.push(rec.finalise());
        }
        Self::merge_duplicates(files)
    }

    /// Merge repeat records for the same `SF` by taking the max of
    /// each counter (some reporters emit one record per test
    /// entry-point and would double-count under naive sum).
    /// Fast-paths the common case where every `SF` is unique to skip
    /// the merge map entirely.
    fn merge_duplicates(files: Vec<LcovFile>) -> Self {
        let mut seen: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::with_capacity(files.len());
        if files.iter().all(|f| seen.insert(f.path.clone())) {
            return Self { files };
        }
        let mut by_path: std::collections::HashMap<PathBuf, usize> =
            std::collections::HashMap::with_capacity(files.len());
        let mut merged: Vec<LcovFile> = Vec::with_capacity(files.len());
        for f in files {
            if let Some(idx) = by_path.get(&f.path).copied() {
                let e = &mut merged[idx];
                e.lines_found = e.lines_found.max(f.lines_found);
                e.lines_hit = e.lines_hit.max(f.lines_hit);
                e.branches_found = e.branches_found.max(f.branches_found);
                e.branches_hit = e.branches_hit.max(f.branches_hit);
            } else {
                by_path.insert(f.path.clone(), merged.len());
                merged.push(f);
            }
        }
        Self { files: merged }
    }
}

/// In-progress record state. Aggregates `DA` / `BRDA` lines as the
/// parser walks the file; finalise resolves `LF` / `LH` either from
/// the explicit summary records or from the `DA` aggregate.
struct LcovRecord {
    path: PathBuf,
    da_total: u32,
    da_hit: u32,
    brda_total: u32,
    brda_hit: u32,
    lf: Option<u32>,
    lh: Option<u32>,
    brf: Option<u32>,
    brh: Option<u32>,
}

impl LcovRecord {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            da_total: 0,
            da_hit: 0,
            brda_total: 0,
            brda_hit: 0,
            lf: None,
            lh: None,
            brf: None,
            brh: None,
        }
    }

    fn note_da(&mut self, value: &str) {
        // `DA:<line>,<hits>[,<checksum>]` — checksum optional.
        let Some((_line, rest)) = value.split_once(',') else {
            return;
        };
        let hits_str = rest.split_once(',').map_or(rest, |(h, _)| h);
        let Ok(hits) = hits_str.parse::<u32>() else {
            return;
        };
        self.da_total = self.da_total.saturating_add(1);
        if hits > 0 {
            self.da_hit = self.da_hit.saturating_add(1);
        }
    }

    fn note_brda(&mut self, value: &str) {
        // `BRDA:<line>,<block>,<branch>,<taken>` — `taken` is `-` for
        // never-evaluated, integer otherwise.
        let Some(taken) = value.rsplit(',').next() else {
            return;
        };
        self.brda_total = self.brda_total.saturating_add(1);
        if taken != "-" && taken != "0" {
            self.brda_hit = self.brda_hit.saturating_add(1);
        }
    }

    fn finalise(self) -> LcovFile {
        LcovFile {
            path: self.path,
            lines_found: self.lf.unwrap_or(self.da_total),
            lines_hit: self.lh.unwrap_or(self.da_hit),
            branches_found: self.brf.unwrap_or(self.brda_total),
            branches_hit: self.brh.unwrap_or(self.brda_hit),
        }
    }
}

/// Resolve an `SF:` path against the project root. lcov reporters
/// disagree on absolute vs relative — `cargo llvm-cov` writes
/// absolute, `nyc` writes relative-to-cwd. We strip the project prefix
/// when present so downstream observers can join with their own
/// project-relative paths.
#[must_use]
pub fn normalise_lcov_path(project: &Path, lcov_path: &Path) -> PathBuf {
    if let Ok(stripped) = lcov_path.strip_prefix(project) {
        return stripped.to_path_buf();
    }
    lcov_path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_two_file_record() {
        let raw = "\
SF:src/foo.rs
DA:1,1
DA:2,1
DA:3,0
LF:3
LH:2
end_of_record
SF:src/bar.rs
DA:1,0
DA:2,0
LF:2
LH:0
end_of_record
";
        let report = LcovReport::parse(raw);
        assert_eq!(report.files.len(), 2);
        assert_eq!(report.files[0].path, PathBuf::from("src/foo.rs"));
        assert_eq!(report.files[0].lines_found, 3);
        assert_eq!(report.files[0].lines_hit, 2);
        assert!((report.files[0].line_coverage_ratio() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(report.files[1].lines_hit, 0);
        assert!(report.files[1].line_coverage_ratio() < 1e-9);
    }

    #[test]
    fn recovers_lf_lh_from_da_when_summary_missing() {
        // `pytest-cov` historically omits `LF`/`LH`; the reader infers
        // from `DA` lines.
        let raw = "\
SF:api/users.py
DA:10,5
DA:11,5
DA:12,0
end_of_record
";
        let report = LcovReport::parse(raw);
        assert_eq!(report.files[0].lines_found, 3);
        assert_eq!(report.files[0].lines_hit, 2);
    }

    #[test]
    fn unknown_records_and_test_names_are_ignored() {
        let raw = "\
TN:my_test_suite
SF:src/lib.rs
VER:1.0
DA:1,1
DA:2,3
LF:2
LH:2
end_of_record
";
        let report = LcovReport::parse(raw);
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].lines_hit, 2);
    }

    #[test]
    fn missing_final_end_of_record_still_emits_record() {
        let raw = "\
SF:src/lib.rs
DA:1,1
LF:1
LH:1
";
        let report = LcovReport::parse(raw);
        assert_eq!(report.files.len(), 1);
    }

    #[test]
    fn duplicate_sf_records_merge_taking_max() {
        // Some reporters emit one record per entry-point. Take the max
        // because adding would risk double-counting when two entry
        // points exercise overlapping lines.
        let raw = "\
SF:src/lib.rs
LF:10
LH:6
end_of_record
SF:src/lib.rs
LF:10
LH:8
end_of_record
";
        let report = LcovReport::parse(raw);
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].lines_hit, 8);
    }

    #[test]
    fn branch_coverage_records_are_aggregated() {
        let raw = "\
SF:src/lib.rs
BRDA:10,0,0,3
BRDA:10,0,1,-
BRDA:11,0,0,0
BRF:3
BRH:1
end_of_record
";
        let report = LcovReport::parse(raw);
        assert_eq!(report.files[0].branches_found, 3);
        assert_eq!(report.files[0].branches_hit, 1);
    }

    #[test]
    fn no_instrumented_lines_returns_full_coverage() {
        let f = LcovFile {
            path: PathBuf::from("src/empty.rs"),
            lines_found: 0,
            lines_hit: 0,
            branches_found: 0,
            branches_hit: 0,
        };
        assert!((f.line_coverage_ratio() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn normalise_lcov_path_strips_project_prefix() {
        let project = PathBuf::from("/work/repo");
        let abs = PathBuf::from("/work/repo/src/lib.rs");
        let rel = PathBuf::from("src/lib.rs");
        assert_eq!(normalise_lcov_path(&project, &abs), rel);
        assert_eq!(normalise_lcov_path(&project, &rel), rel);
    }
}
