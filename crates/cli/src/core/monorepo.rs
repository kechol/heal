//! Detect signals that a project declares workspaces. Used by `heal
//! init` to print a hint and by the `heal-config` skill (via
//! `heal init --json`) to know when to run its workspace-declaration
//! phase.
//!
//! For Cargo and npm/yarn manifests we additionally enumerate the
//! declared member paths (literal entries plus simple `pkg/*` globs)
//! and run a lightweight LOC scan over each so the init summary can
//! show `path (primary_language)` per workspace. Manifests we can't
//! parse without extra dependencies (pnpm yaml, go work, Nx / Turbo)
//! still register as a presence signal with no member list.

use std::path::Path;

use serde::Serialize;

use crate::core::config::Config;
use crate::observer::loc::LocObserver;

/// One detected workspace-declaration signal.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MonorepoSignal {
    /// File whose presence (and shape) triggered the signal, relative
    /// to the project root. Stable identifier the renderer + skills
    /// match against.
    pub manifest: String,
    /// Short human-readable description of what was detected
    /// ("workspaces array", "[workspace] members"). Goes straight into
    /// the init summary so it must be parseable in one glance.
    pub kind: String,
    /// Member directories resolved from the manifest, relative to the
    /// project root. Empty when the manifest only signals intent
    /// (pnpm yaml, go.work, Nx / Turbo) or when no members could be
    /// expanded — callers fall back to the `manifest` line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<DetectedWorkspace>,
}

/// One member directory enumerated from a workspace manifest.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DetectedWorkspace {
    /// Path relative to the project root, slash-separated. Same shape
    /// the user would put under `[[project.workspaces]].path`.
    pub path: String,
    /// Auto-detected primary language for this member directory.
    /// `None` when the directory contained no source tokei
    /// recognises.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_language: Option<String>,
}

/// Scan the project root for workspace manifests. Returns an empty vec
/// for solo-package projects. Order is stable (manifest filename
/// alphabetic) so the renderer's output is reproducible.
#[must_use]
pub fn detect(project_root: &Path) -> Vec<MonorepoSignal> {
    let mut hits = Vec::new();
    if let Some(s) = detect_npm_workspaces(project_root) {
        hits.push(s);
    }
    if project_root.join("pnpm-workspace.yaml").exists() {
        hits.push(MonorepoSignal {
            manifest: "pnpm-workspace.yaml".into(),
            kind: "pnpm packages".into(),
            members: Vec::new(),
        });
    }
    if let Some(s) = detect_cargo_workspace(project_root) {
        hits.push(s);
    }
    if project_root.join("go.work").exists() {
        hits.push(MonorepoSignal {
            manifest: "go.work".into(),
            kind: "go work members".into(),
            members: Vec::new(),
        });
    }
    if project_root.join("nx.json").exists() {
        hits.push(MonorepoSignal {
            manifest: "nx.json".into(),
            kind: "Nx workspace".into(),
            members: Vec::new(),
        });
    }
    if project_root.join("turbo.json").exists() {
        hits.push(MonorepoSignal {
            manifest: "turbo.json".into(),
            kind: "Turborepo workspace".into(),
            members: Vec::new(),
        });
    }
    hits.sort_by(|a, b| a.manifest.cmp(&b.manifest));
    hits.dedup_by(|a, b| a.manifest == b.manifest);
    hits
}

/// Decorate every signal's `members` with the auto-detected primary
/// language. Splitting the LOC scan out of `detect` keeps detection
/// pure (and trivially testable); the renderer drives this only for
/// manifests whose member list was successfully enumerated.
pub fn enrich_with_languages(project_root: &Path, cfg: &Config, signals: &mut [MonorepoSignal]) {
    let observer = LocObserver::from_config(cfg);
    for sig in signals {
        for member in &mut sig.members {
            let dir = project_root.join(&member.path);
            if !dir.is_dir() {
                continue;
            }
            member.primary_language = observer.scan(&dir).primary;
        }
    }
}

fn detect_npm_workspaces(project_root: &Path) -> Option<MonorepoSignal> {
    let body = std::fs::read_to_string(project_root.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    // npm/yarn allow either `"workspaces": ["pkg/*"]` or
    // `"workspaces": {"packages": ["pkg/*"]}`. Treat both as a hit.
    let ws = json.get("workspaces")?;
    let entries: Vec<&str> = match ws {
        serde_json::Value::Array(arr) => arr.iter().filter_map(serde_json::Value::as_str).collect(),
        serde_json::Value::Object(obj) => obj
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if entries.is_empty() {
        return None;
    }
    let members = expand_member_specs(project_root, &entries);
    Some(MonorepoSignal {
        manifest: "package.json".into(),
        kind: "workspaces array".into(),
        members,
    })
}

fn detect_cargo_workspace(project_root: &Path) -> Option<MonorepoSignal> {
    let body = std::fs::read_to_string(project_root.join("Cargo.toml")).ok()?;
    let toml: toml::Value = toml::from_str(&body).ok()?;
    // Either `[workspace] members = [...]` (canonical) or the bare
    // `[workspace]` table with no members (also a workspace, just empty).
    // Both count — the user is signaling monorepo intent.
    let workspace = toml.get("workspace")?;
    let entries: Vec<&str> = workspace
        .get("members")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();
    let members = expand_member_specs(project_root, &entries);
    Some(MonorepoSignal {
        manifest: "Cargo.toml".into(),
        kind: "[workspace] members".into(),
        members,
    })
}

/// Resolve workspace member specs (literal paths plus the common
/// `prefix/*` glob shape) against the project root. Returns the ones
/// that exist on disk as directories. More elaborate glob syntax
/// (`**`, character classes, `!` negations) is intentionally not
/// supported — workspaces declared that way still surface as
/// presence-only signals via the `manifest` line.
fn expand_member_specs(project_root: &Path, specs: &[&str]) -> Vec<DetectedWorkspace> {
    let mut out: Vec<DetectedWorkspace> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for spec in specs {
        for path in expand_one_spec(project_root, spec) {
            if seen.insert(path.clone()) {
                out.push(DetectedWorkspace {
                    path,
                    primary_language: None,
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn expand_one_spec(project_root: &Path, spec: &str) -> Vec<String> {
    if spec.is_empty() {
        return Vec::new();
    }
    if let Some((prefix, rest)) = spec.split_once('*') {
        // Only the simple `prefix/*` shape is supported. Anything more
        // elaborate (`**`, second wildcard, etc.) is dropped to keep
        // the binary glob-dep-free.
        if !rest.is_empty() && rest != "/" {
            return Vec::new();
        }
        let trimmed = prefix.trim_end_matches('/');
        let parent = if trimmed.is_empty() {
            project_root.to_path_buf()
        } else {
            project_root.join(trimmed)
        };
        let Ok(entries) = std::fs::read_dir(&parent) else {
            return Vec::new();
        };
        let mut hits = Vec::new();
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let joined = if trimmed.is_empty() {
                name
            } else {
                format!("{trimmed}/{name}")
            };
            hits.push(joined);
        }
        hits
    } else {
        let candidate = project_root.join(spec);
        if candidate.is_dir() {
            vec![spec.trim_end_matches('/').to_owned()]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn empty_project_yields_no_signals() {
        let dir = TempDir::new().unwrap();
        assert!(detect(dir.path()).is_empty());
    }

    #[test]
    fn package_json_with_workspaces_array_detected() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "package.json",
            r#"{"name":"r","workspaces":["packages/*"]}"#,
        );
        let s = detect(dir.path());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].manifest, "package.json");
    }

    #[test]
    fn package_json_with_workspaces_object_detected() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "package.json",
            r#"{"name":"r","workspaces":{"packages":["pkg/*"]}}"#,
        );
        assert_eq!(detect(dir.path()).len(), 1);
    }

    #[test]
    fn package_json_without_workspaces_ignored() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "package.json", r#"{"name":"r"}"#);
        assert!(detect(dir.path()).is_empty());
    }

    #[test]
    fn package_json_with_empty_workspaces_ignored() {
        // An empty workspaces list is not a monorepo — would yield
        // false-positive hints on solo packages that left the key in.
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "package.json",
            r#"{"name":"r","workspaces":[]}"#,
        );
        assert!(detect(dir.path()).is_empty());
    }

    #[test]
    fn cargo_workspace_detected() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n",
        );
        let s = detect(dir.path());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].manifest, "Cargo.toml");
    }

    #[test]
    fn cargo_solo_package_ignored() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "Cargo.toml",
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"\n",
        );
        assert!(detect(dir.path()).is_empty());
    }

    #[test]
    fn pnpm_yaml_detected_by_presence() {
        // No yaml parser; presence alone is enough to suggest the skill.
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "pnpm-workspace.yaml",
            "packages:\n  - 'pkg/*'\n",
        );
        assert_eq!(detect(dir.path()).len(), 1);
    }

    #[test]
    fn malformed_package_json_is_silent() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "package.json", "not json");
        assert!(detect(dir.path()).is_empty());
    }

    #[test]
    fn output_is_alphabetic_by_manifest() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "package.json",
            r#"{"name":"r","workspaces":["pkg/*"]}"#,
        );
        write(
            dir.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n",
        );
        let signals = detect(dir.path());
        let names: Vec<&str> = signals.iter().map(|s| s.manifest.as_str()).collect();
        assert_eq!(names, vec!["Cargo.toml", "package.json"]);
    }

    #[test]
    fn cargo_member_glob_expands_to_subdirs() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("crates/cli")).unwrap();
        std::fs::create_dir_all(dir.path().join("crates/core")).unwrap();
        write(
            dir.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n",
        );
        let signals = detect(dir.path());
        assert_eq!(signals.len(), 1);
        let paths: Vec<&str> = signals[0].members.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, vec!["crates/cli", "crates/core"]);
    }

    #[test]
    fn cargo_literal_member_resolved_when_dir_exists() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("crates/cli")).unwrap();
        write(
            dir.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/cli\"]\n",
        );
        let signals = detect(dir.path());
        assert_eq!(signals[0].members.len(), 1);
        assert_eq!(signals[0].members[0].path, "crates/cli");
    }

    #[test]
    fn cargo_workspace_without_members_table_has_empty_member_list() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "Cargo.toml", "[workspace]\n");
        let signals = detect(dir.path());
        assert_eq!(signals.len(), 1);
        assert!(signals[0].members.is_empty());
    }

    #[test]
    fn npm_member_glob_expands_to_subdirs() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("packages/web")).unwrap();
        std::fs::create_dir_all(dir.path().join("packages/api")).unwrap();
        write(
            dir.path(),
            "package.json",
            r#"{"name":"r","workspaces":["packages/*"]}"#,
        );
        let signals = detect(dir.path());
        let paths: Vec<&str> = signals[0].members.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, vec!["packages/api", "packages/web"]);
    }

    #[test]
    fn enrich_with_languages_fills_primary() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("crates/cli/src")).unwrap();
        write(
            dir.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/cli\"]\n",
        );
        std::fs::write(dir.path().join("crates/cli/src/main.rs"), "fn main() {}\n").unwrap();
        let mut signals = detect(dir.path());
        let cfg = Config::default();
        enrich_with_languages(dir.path(), &cfg, &mut signals);
        assert_eq!(signals[0].members[0].path, "crates/cli");
        assert_eq!(
            signals[0].members[0].primary_language.as_deref(),
            Some("Rust"),
        );
    }
}
