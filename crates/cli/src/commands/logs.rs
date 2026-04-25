use std::path::Path;

use anyhow::Result;
use heal_core::history::HistoryReader;
use heal_core::HealPaths;

pub fn run(project: &Path, since: Option<&str>, filter: Option<&str>) -> Result<()> {
    let paths = HealPaths::new(project);
    let reader = HistoryReader::new(paths.history_dir());

    let since_dt = since
        .map(|s| chrono::DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&chrono::Utc)))
        .transpose()?;

    for snapshot in reader.try_iter()? {
        let snapshot = snapshot?;
        if let Some(cutoff) = since_dt {
            if snapshot.timestamp < cutoff {
                continue;
            }
        }
        if let Some(f) = filter {
            if snapshot.event != f {
                continue;
            }
        }
        println!("{}", serde_json::to_string(&snapshot)?);
    }
    Ok(())
}
