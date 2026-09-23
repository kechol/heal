//! `heal auth jev set | status | clear` — manage the `TypeSafe` API key.
//!
//! The key lives outside the project: in the environment
//! (`TYPESAFE_API_KEY`) or in the per-user `credentials.toml`. `status`
//! is the second of the two places HEAL may open a network connection:
//! it confirms the key against `GET /v1/models`.

use std::io::{BufRead, IsTerminal};
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::Serialize;

use crate::core::config::{load_from_project, SemanticConfig};
use crate::semantic::client::model_listed;
use crate::semantic::credentials::{self, default_credentials_path, resolve};

fn credentials_file() -> Result<std::path::PathBuf> {
    default_credentials_path().ok_or_else(|| {
        anyhow!("cannot locate a user config directory (set HOME or XDG_CONFIG_HOME)")
    })
}

/// Read one key from stdin. Piping (`… | heal auth jev set`) keeps it out
/// of shell history; on a terminal the input is echoed, so say so.
pub fn run_set(json: bool) -> Result<()> {
    let path = credentials_file()?;
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("Paste your TypeSafe API key and press Enter (input is visible; pipe it in to avoid that):");
    }
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    credentials::store(&path, &line).map_err(anyhow::Error::msg)?;
    if json {
        super::emit_json(&serde_json::json!({"stored": true, "path": path}));
    } else {
        println!("Stored the Jev API key in {} (mode 600).", path.display());
        if credentials::API_KEY_VARS
            .iter()
            .any(|v| std::env::var(v).is_ok_and(|s| !s.trim().is_empty()))
        {
            println!("Note: an environment variable is set and takes precedence over this file.");
        }
    }
    Ok(())
}

pub fn run_clear(json: bool) -> Result<()> {
    let path = credentials_file()?;
    let removed = credentials::clear(&path).map_err(anyhow::Error::msg)?;
    if json {
        super::emit_json(&serde_json::json!({"removed": removed, "path": path}));
    } else if removed {
        println!("Removed {}.", path.display());
    } else {
        println!("No stored key at {}.", path.display());
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct StatusReport {
    configured: bool,
    source: Option<String>,
    key: Option<String>,
    model: String,
    reachable: Option<bool>,
    model_available: Option<bool>,
    error: Option<String>,
}

/// Show where the key comes from, then confirm it against the API.
/// Exits non-zero when no key is configured or the check fails, so CI can
/// gate on it.
pub fn run_status(project: &Path, json: bool, offline: bool) -> Result<()> {
    let model = load_from_project(project).map_or_else(
        |_| SemanticConfig::DEFAULT_MODEL.to_owned(),
        |c| c.features.semantic.model,
    );
    let file = default_credentials_path();
    let resolved = resolve(file.as_deref()).map_err(anyhow::Error::msg)?;
    let mut report = StatusReport {
        configured: resolved.is_some(),
        source: resolved.as_ref().map(|r| r.source.to_string()),
        key: resolved.as_ref().map(credentials::ResolvedKey::masked),
        model: model.clone(),
        reachable: None,
        model_available: None,
        error: None,
    };
    let mut listed = Vec::new();
    if resolved.is_some() && !offline {
        match super::semantic::build_client(&model)
            .and_then(|c| c.list_models().map_err(|e| anyhow!("{e}")))
        {
            Ok(models) => {
                report.reachable = Some(true);
                // `None` rather than `false` when unlisted: the listing
                // omits pinned versions, so it cannot rule a model out.
                report.model_available = model_listed(&models, &model).then_some(true);
                listed = models;
            }
            Err(e) => {
                report.reachable = Some(false);
                report.error = Some(e.to_string());
            }
        }
    }
    let ok = report.configured && report.reachable != Some(false);
    if json {
        super::emit_json(&report);
    } else {
        match (&report.source, &report.key) {
            (Some(src), Some(key)) => println!("Jev API key: {key} (from {src})"),
            _ => println!(
                "Jev API key: not configured. Export TYPESAFE_API_KEY or run `heal auth jev set`."
            ),
        }
        match report.reachable {
            Some(true) if report.model_available == Some(true) => {
                println!("API: reachable; model `{model}` available");
            }
            Some(true) => println!(
                "API: reachable; key accepted. {}",
                super::semantic::unlisted_model_note(&model, &listed)
            ),
            Some(false) => println!(
                "API: check failed: {}",
                report.error.as_deref().unwrap_or("unknown error")
            ),
            None if offline => println!("API: not checked (--offline)"),
            None => {}
        }
    }
    if !ok {
        std::process::exit(super::semantic::SEMANTIC_SETUP_EXIT_CODE);
    }
    Ok(())
}
