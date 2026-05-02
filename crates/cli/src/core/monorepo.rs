//! Detect signals that a project is a monorepo. Used by `heal init` to
//! print a hint and by the `heal-config` skill (via `heal init --json`)
//! to know when to run its workspace-declaration phase.
//!
//! Deliberately *only* detects manifest **presence + intent** — it does
//! not enumerate the actual workspace member directories. The skill
//! does that with its richer toolset (Read / Bash / Glob), and dropping
//! that responsibility here keeps the binary free of glob and yaml
//! dependencies for what is essentially a one-line operator hint.

use std::path::Path;

use serde::Serialize;

/// One detected monorepo signal.
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
}

/// Scan the project root for monorepo manifests. Returns an empty vec
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
        });
    }
    if let Some(s) = detect_cargo_workspace(project_root) {
        hits.push(s);
    }
    if project_root.join("go.work").exists() {
        hits.push(MonorepoSignal {
            manifest: "go.work".into(),
            kind: "go work members".into(),
        });
    }
    if project_root.join("nx.json").exists() {
        hits.push(MonorepoSignal {
            manifest: "nx.json".into(),
            kind: "Nx workspace".into(),
        });
    }
    if project_root.join("turbo.json").exists() {
        hits.push(MonorepoSignal {
            manifest: "turbo.json".into(),
            kind: "Turborepo workspace".into(),
        });
    }
    hits.sort_by(|a, b| a.manifest.cmp(&b.manifest));
    hits.dedup_by(|a, b| a.manifest == b.manifest);
    hits
}

fn detect_npm_workspaces(project_root: &Path) -> Option<MonorepoSignal> {
    let body = std::fs::read_to_string(project_root.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    // npm/yarn allow either `"workspaces": ["pkg/*"]` or
    // `"workspaces": {"packages": ["pkg/*"]}`. Treat both as a hit.
    let ws = json.get("workspaces")?;
    let has_entries = match ws {
        serde_json::Value::Array(arr) => !arr.is_empty(),
        serde_json::Value::Object(obj) => obj
            .get("packages")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|a| !a.is_empty()),
        _ => false,
    };
    has_entries.then(|| MonorepoSignal {
        manifest: "package.json".into(),
        kind: "workspaces array".into(),
    })
}

fn detect_cargo_workspace(project_root: &Path) -> Option<MonorepoSignal> {
    let body = std::fs::read_to_string(project_root.join("Cargo.toml")).ok()?;
    let toml: toml::Value = toml::from_str(&body).ok()?;
    // Either `[workspace] members = [...]` (canonical) or the bare
    // `[workspace]` table with no members (also a workspace, just empty).
    // Both count — the user is signaling monorepo intent.
    toml.get("workspace").map(|_| MonorepoSignal {
        manifest: "Cargo.toml".into(),
        kind: "[workspace] members".into(),
    })
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
}
