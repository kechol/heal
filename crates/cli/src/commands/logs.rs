//! `heal checks` — newest-first browser over `.heal/checks/` records.
//! The current findings live under `heal status` (which writes the
//! cache); this command is for operators inspecting the historical
//! `CheckRecord` log.

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::cli::ChecksFilters;
use crate::core::check_cache::{iter_records, CheckRecord, CheckRecordSummary};
use crate::core::HealPaths;

pub fn run_checks(project: &Path, args: &ChecksFilters) -> Result<()> {
    let paths = HealPaths::new(project);
    let since_dt = parse_since(args.since.as_deref())?;
    let mut records: Vec<CheckRecord> = iter_records(&paths.checks_dir())?
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| since_dt.is_none_or(|cutoff| r.started_at >= cutoff))
        .collect();
    if let Some(n) = args.limit {
        records.truncate(n);
    }
    if args.json {
        let payload: Vec<CheckRecordSummary> =
            records.iter().map(CheckRecordSummary::from).collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .expect("CheckRecordSummary serialization is infallible")
        );
        return Ok(());
    }
    if records.is_empty() {
        println!(
            "no records yet at {} — run `heal status`",
            paths.checks_dir().display()
        );
        return Ok(());
    }
    for r in &records {
        let counts = &r.severity_counts;
        println!(
            "{}  {}  head={}  findings={}  C {}  H {}  M {}",
            r.check_id,
            r.started_at.format("%Y-%m-%d %H:%M"),
            r.head_sha.as_deref().unwrap_or("∅"),
            r.findings.len(),
            counts.critical,
            counts.high,
            counts.medium,
        );
    }
    Ok(())
}

fn parse_since(since: Option<&str>) -> Result<Option<DateTime<Utc>>> {
    since
        .map(|s| {
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(anyhow::Error::from)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn run_checks_handles_empty_dir() {
        let dir = TempDir::new().unwrap();
        let paths = HealPaths::new(dir.path());
        paths.ensure().unwrap();
        run_checks(
            dir.path(),
            &ChecksFilters {
                since: None,
                limit: None,
                json: false,
            },
        )
        .unwrap();
    }
}
