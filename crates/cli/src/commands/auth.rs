//! `heal auth <provider> set | status | clear` — manage a provider's API
//! key.
//!
//! A key lives outside the project: in the environment
//! (`TYPESAFE_API_KEY`, `ANTHROPIC_API_KEY`) or in the per-user
//! `credentials.toml`. `status` is the second of the two places HEAL may
//! reach the network: it confirms the credentials against the provider.

use std::io::{BufRead, IsTerminal};
use std::path::Path;

use anyhow::{anyhow, Result};
use serde::Serialize;

use crate::core::config::{load_from_project, SemanticProvider};
use crate::semantic::credentials::{self, default_credentials_path, resolve_backend, Backend};

fn credentials_file() -> Result<std::path::PathBuf> {
    default_credentials_path().ok_or_else(|| {
        anyhow!("cannot locate a user config directory (set HOME or XDG_CONFIG_HOME)")
    })
}

fn provider_name(provider: SemanticProvider) -> &'static str {
    match provider {
        SemanticProvider::Jev => "Jev",
        SemanticProvider::Claude => "Claude",
    }
}

/// Read one key from stdin. Piping (`… | heal auth <provider> set`) keeps it
/// out of shell history; on a terminal the input is echoed, so say so.
pub fn run_set(provider: SemanticProvider, json: bool) -> Result<()> {
    let path = credentials_file()?;
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!(
            "Paste your {} API key and press Enter (input is visible; pipe it in to avoid that):",
            provider_name(provider)
        );
    }
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    credentials::store(&path, provider, &line).map_err(anyhow::Error::msg)?;
    if json {
        super::emit_json(&serde_json::json!({"stored": true, "path": path}));
    } else {
        println!(
            "Stored the {} API key in {} (mode 600).",
            provider_name(provider),
            path.display()
        );
        if credentials::api_key_vars(provider)
            .iter()
            .any(|v| std::env::var(v).is_ok_and(|s| !s.trim().is_empty()))
        {
            println!("Note: an environment variable is set and takes precedence over this file.");
        }
    }
    Ok(())
}

pub fn run_clear(provider: SemanticProvider, json: bool) -> Result<()> {
    let path = credentials_file()?;
    let removed = credentials::clear(&path, provider).map_err(anyhow::Error::msg)?;
    if json {
        super::emit_json(&serde_json::json!({"removed": removed, "path": path}));
    } else if removed {
        println!(
            "Removed the {} API key from {}.",
            provider_name(provider),
            path.display()
        );
    } else {
        println!(
            "No stored {} API key at {}.",
            provider_name(provider),
            path.display()
        );
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct StatusReport {
    configured: bool,
    source: Option<String>,
    key: Option<String>,
    model: String,
    /// How `heal semantic ask` would reach the provider: `api` or `claude_cli`.
    backend: Option<&'static str>,
    reachable: Option<bool>,
    model_available: Option<bool>,
    error: Option<String>,
}

/// Show where the credentials come from, then confirm them against the
/// provider. Exits non-zero when none are configured or the check fails, so
/// CI can gate on it.
pub fn run_status(
    project: &Path,
    provider: SemanticProvider,
    json: bool,
    offline: bool,
) -> Result<()> {
    let name = provider_name(provider);
    let model = load_from_project(project)
        .ok()
        .map(|c| c.features.semantic.model)
        .filter(|m| SemanticProvider::of_model(m) == provider)
        .unwrap_or_else(|| provider.default_model().to_owned());
    let file = default_credentials_path();
    let backend = resolve_backend(provider, file.as_deref()).map_err(anyhow::Error::msg)?;
    let key = match &backend {
        Some(Backend::Api(key)) => Some(key),
        _ => None,
    };
    let via_cli = matches!(backend, Some(Backend::ClaudeCli));
    let mut report = StatusReport {
        configured: backend.is_some(),
        source: key.map(|r| r.source.to_string()),
        key: key.map(credentials::ResolvedKey::masked),
        model: model.clone(),
        backend: backend.as_ref().map(Backend::id),
        reachable: None,
        model_available: None,
        error: None,
    };
    let mut note = None;
    if report.configured && !offline {
        match super::semantic::build_client(&model)
            .and_then(|c| c.check().map_err(|e| anyhow!("{e}")))
        {
            Ok(check) => {
                report.reachable = Some(true);
                report.model_available = check.model_available;
                note = Some(check.note);
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
            (Some(src), Some(key)) => println!("{name} API key: {key} (from {src})"),
            _ if via_cli => println!("{name} API key: none; using the claude CLI's sign-in"),
            _ => println!(
                "{name} API key: not configured. {}",
                match provider {
                    SemanticProvider::Jev => "Export TYPESAFE_API_KEY, or run `heal auth jev set`.",
                    SemanticProvider::Claude =>
                        "Export ANTHROPIC_API_KEY, run `heal auth claude set`, or install and \
                         sign in to the `claude` CLI.",
                }
            ),
        }
        match report.reachable {
            Some(true) => println!("{}", note.as_deref().unwrap_or_default()),
            Some(false) => println!(
                "Check failed: {}",
                report.error.as_deref().unwrap_or("unknown error")
            ),
            None if offline => println!("Not checked (--offline)"),
            None => {}
        }
    }
    if !ok {
        std::process::exit(super::semantic::SEMANTIC_SETUP_EXIT_CODE);
    }
    Ok(())
}
