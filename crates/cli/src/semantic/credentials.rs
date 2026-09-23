//! Resolving the `TypeSafe` API key.
//!
//! Lookup order: `TYPESAFE_API_KEY`, then `TYPESAFEAI_API_KEY` (the same
//! pair jev-lint and jev-lexer read, so one exported key serves every
//! tool), then the per-user file `<config dir>/heal/credentials.toml`.
//!
//! The key never lives under `.heal/`: `config.toml` is a tracked team
//! contract and a secret does not belong in version control. The user
//! file must not be readable by group or others; a looser mode is an
//! error rather than a warning, since the file only ever holds a secret.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const API_KEY_VARS: [&str; 2] = ["TYPESAFE_API_KEY", "TYPESAFEAI_API_KEY"];

/// Where the resolved key came from. Printed by `heal auth jev status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    Env(&'static str),
    File(PathBuf),
}

impl std::fmt::Display for KeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Env(var) => write!(f, "environment variable {var}"),
            Self::File(path) => write!(f, "{}", path.display()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedKey {
    pub key: String,
    pub source: KeySource,
}

impl ResolvedKey {
    /// First four and last four characters; enough to tell two keys apart
    /// without printing a usable secret.
    #[must_use]
    pub fn masked(&self) -> String {
        mask(&self.key)
    }
}

#[must_use]
pub fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialsFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jev: Option<JevCredentials>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JevCredentials {
    api_key: String,
}

/// `$XDG_CONFIG_HOME/heal/credentials.toml`, else
/// `~/.config/heal/credentials.toml` (`%APPDATA%\heal\` on Windows).
#[must_use]
pub fn default_credentials_path() -> Option<PathBuf> {
    credentials_path_from(|name| std::env::var_os(name).map(PathBuf::from))
}

fn credentials_path_from(env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let base = env("XDG_CONFIG_HOME")
        .filter(|p| p.is_absolute())
        .or_else(|| {
            if cfg!(windows) {
                env("APPDATA")
            } else {
                env("HOME").map(|h| h.join(".config"))
            }
        })?;
    Some(base.join("heal").join("credentials.toml"))
}

/// Resolve the key from the environment, then from `file`.
pub fn resolve(file: Option<&Path>) -> Result<Option<ResolvedKey>, String> {
    resolve_with(|name| std::env::var(name).ok(), file)
}

fn resolve_with(
    env: impl Fn(&str) -> Option<String>,
    file: Option<&Path>,
) -> Result<Option<ResolvedKey>, String> {
    for var in API_KEY_VARS {
        if let Some(v) = env(var)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
        {
            return Ok(Some(ResolvedKey {
                key: v,
                source: KeySource::Env(var),
            }));
        }
    }
    let Some(path) = file else {
        return Ok(None);
    };
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    check_private(path)?;
    let parsed: CredentialsFile =
        toml::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(parsed
        .jev
        .map(|j| j.api_key.trim().to_owned())
        .filter(|k| !k.is_empty())
        .map(|key| ResolvedKey {
            key,
            source: KeySource::File(path.to_path_buf()),
        }))
}

#[cfg(unix)]
fn check_private(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(format!(
            "{} is readable by group or others (mode {:o}); run `chmod 600` on it or `heal auth jev set` to rewrite it",
            path.display(),
            mode & 0o777
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Write the key to `path` with mode 0600, replacing any previous key.
pub fn store(path: &Path, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("refusing to store an empty key".to_owned());
    }
    let body = toml::to_string(&CredentialsFile {
        jev: Some(JevCredentials {
            api_key: key.to_owned(),
        }),
    })
    .map_err(|e| e.to_string())?;
    write_private(path, body.as_bytes())
}

/// Remove the Jev key. Returns whether a key was present.
pub fn clear(path: &Path) -> Result<bool, String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write_private(path: &Path, body: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".heal-cred-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o600));
    }
    let mut tmp = builder
        .tempfile_in(parent)
        .map_err(|e| format!("{}: {e}", parent.display()))?;
    tmp.write_all(body)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    tmp.persist(path)
        .map_err(|e| format!("{}: {}", path.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_wins_over_file_and_primary_var_wins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.toml");
        store(&path, "file-key-123456").unwrap();
        let env = |n: &str| match n {
            "TYPESAFE_API_KEY" => Some("primary".to_owned()),
            "TYPESAFEAI_API_KEY" => Some("fallback".to_owned()),
            _ => None,
        };
        let r = resolve_with(env, Some(&path)).unwrap().unwrap();
        assert_eq!(r.key, "primary");
        assert_eq!(r.source, KeySource::Env("TYPESAFE_API_KEY"));

        let r = resolve_with(|_| None, Some(&path)).unwrap().unwrap();
        assert_eq!(r.key, "file-key-123456");
        assert!(matches!(r.source, KeySource::File(_)));
    }

    #[test]
    fn blank_env_values_are_ignored() {
        let env = |n: &str| (n == "TYPESAFE_API_KEY").then(|| "  ".to_owned());
        assert!(resolve_with(env, None).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn stored_file_is_private_and_loose_mode_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heal/credentials.toml");
        store(&path, "abcdefghijkl").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve_with(|_| None, Some(&path)).is_err());
    }

    #[test]
    fn clear_reports_whether_a_key_existed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        assert!(!clear(&path).unwrap());
        store(&path, "abcdefghijkl").unwrap();
        assert!(clear(&path).unwrap());
    }

    #[test]
    fn mask_hides_the_middle() {
        assert_eq!(mask("abcdefghijkl"), "abcd…ijkl");
        assert_eq!(mask("short"), "*****");
    }

    #[test]
    fn credentials_path_prefers_absolute_xdg() {
        let env = |n: &str| match n {
            "XDG_CONFIG_HOME" => Some(PathBuf::from("/x")),
            "HOME" => Some(PathBuf::from("/h")),
            _ => None,
        };
        assert_eq!(
            credentials_path_from(env).unwrap(),
            PathBuf::from("/x/heal/credentials.toml")
        );
        let env = |n: &str| match n {
            "XDG_CONFIG_HOME" => Some(PathBuf::from("rel")),
            "HOME" => Some(PathBuf::from("/h")),
            "APPDATA" => Some(PathBuf::from("/a")),
            _ => None,
        };
        let expected = if cfg!(windows) {
            PathBuf::from("/a/heal/credentials.toml")
        } else {
            PathBuf::from("/h/.config/heal/credentials.toml")
        };
        assert_eq!(credentials_path_from(env).unwrap(), expected);
    }
}
