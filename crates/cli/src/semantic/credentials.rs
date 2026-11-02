//! Resolving the provider's API key.
//!
//! Lookup order for Jev: `TYPESAFE_API_KEY`, then `TYPESAFEAI_API_KEY` (the
//! same pair jev-lint and jev-lexer read, so one exported key serves every
//! tool). For Claude: `ANTHROPIC_API_KEY`. Then the per-user file
//! `<config dir>/heal/credentials.toml`, one table per provider.
//!
//! A key never lives under `.heal/`: `config.toml` is a tracked team
//! contract and a secret does not belong in version control. The user
//! file must not be readable by group or others; a looser mode is an
//! error rather than a warning, since the file only ever holds a secret.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::config::SemanticProvider;
use crate::semantic::claude_cli;

#[must_use]
pub const fn api_key_vars(provider: SemanticProvider) -> &'static [&'static str] {
    match provider {
        SemanticProvider::Jev => &["TYPESAFE_API_KEY", "TYPESAFEAI_API_KEY"],
        SemanticProvider::Claude => &["ANTHROPIC_API_KEY"],
    }
}

/// Where the resolved key came from. Printed by `heal auth <provider> status`.
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

#[derive(Clone)]
pub struct ResolvedKey {
    pub key: String,
    pub source: KeySource,
}

/// Masked, so a stray `{:?}` or panic message never prints the key.
impl std::fmt::Debug for ResolvedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedKey")
            .field("key", &self.masked())
            .field("source", &self.source)
            .finish()
    }
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

/// How `heal semantic ask` reaches the provider.
#[derive(Debug)]
pub enum Backend {
    Api(ResolvedKey),
    /// Claude without a key: the `claude` CLI's own sign-in.
    ClaudeCli,
}

impl Backend {
    /// The `backend` field of `heal auth <provider> status --json`.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        match self {
            Self::Api(_) => "api",
            Self::ClaudeCli => "claude_cli",
        }
    }
}

/// A key wins; Claude without one falls back to the `claude` CLI when it
/// is on `PATH`.
pub fn resolve_backend(
    provider: SemanticProvider,
    file: Option<&Path>,
) -> Result<Option<Backend>, String> {
    Ok(match resolve(provider, file)? {
        Some(key) => Some(Backend::Api(key)),
        None if provider == SemanticProvider::Claude && claude_cli::available() => {
            Some(Backend::ClaudeCli)
        }
        None => None,
    })
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialsFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jev: Option<ProviderCredentials>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    claude: Option<ProviderCredentials>,
}

impl CredentialsFile {
    fn slot(&mut self, provider: SemanticProvider) -> &mut Option<ProviderCredentials> {
        match provider {
            SemanticProvider::Jev => &mut self.jev,
            SemanticProvider::Claude => &mut self.claude,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderCredentials {
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

/// Resolve `provider`'s key from the environment, then from `file`.
pub fn resolve(
    provider: SemanticProvider,
    file: Option<&Path>,
) -> Result<Option<ResolvedKey>, String> {
    resolve_with(provider, |name| std::env::var(name).ok(), file)
}

fn resolve_with(
    provider: SemanticProvider,
    env: impl Fn(&str) -> Option<String>,
    file: Option<&Path>,
) -> Result<Option<ResolvedKey>, String> {
    for var in api_key_vars(provider) {
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
    let mut parsed: CredentialsFile = toml::from_str(&raw).map_err(|e| {
        // Neither the error's Display (it quotes the offending line) nor its
        // message (serde's type errors quote the value) may be printed: the
        // offending text is usually the key itself.
        let line = e.span().map_or(1, |s| {
            raw[..s.start.min(raw.len())].matches('\n').count() + 1
        });
        format!(
            "{}: line {line}: not a valid credentials file; expected `[{id}]` with \
             `api_key = \"<key>\"` (run `heal auth {id} set` to rewrite it)",
            path.display(),
            id = provider.id()
        )
    })?;
    Ok(parsed
        .slot(provider)
        .take()
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
            "{} is readable by group or others (mode {:o}); run `chmod 600` on it",
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

/// Read the file for an update. An unreadable or malformed file is treated
/// as empty: the point of `set` and `clear` is to repair it.
fn read_for_update(path: &Path) -> CredentialsFile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| toml::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Write `provider`'s key to `path` with mode 0600, replacing its previous
/// key and keeping the other provider's.
pub fn store(path: &Path, provider: SemanticProvider, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("refusing to store an empty key".to_owned());
    }
    let mut file = read_for_update(path);
    *file.slot(provider) = Some(ProviderCredentials {
        api_key: key.to_owned(),
    });
    let body = toml::to_string(&file).map_err(|e| e.to_string())?;
    write_private(path, body.as_bytes())
}

/// Remove `provider`'s key, deleting the file once it holds no key. Returns
/// whether a key was present.
pub fn clear(path: &Path, provider: SemanticProvider) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let mut file = read_for_update(path);
    let removed = file.slot(provider).take().is_some();
    if file.jev.is_none() && file.claude.is_none() {
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    } else if removed {
        let body = toml::to_string(&file).map_err(|e| e.to_string())?;
        write_private(path, body.as_bytes())?;
    }
    Ok(removed)
}

fn write_private(path: &Path, body: &[u8]) -> Result<(), String> {
    crate::core::fs::atomic_write_mode(path, body, 0o600).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::SemanticProvider::{Claude, Jev};

    #[test]
    fn env_wins_over_file_and_primary_var_wins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.toml");
        store(&path, Jev, "file-key-123456").unwrap();
        let env = |n: &str| match n {
            "TYPESAFE_API_KEY" => Some("primary".to_owned()),
            "TYPESAFEAI_API_KEY" => Some("fallback".to_owned()),
            _ => None,
        };
        let r = resolve_with(Jev, env, Some(&path)).unwrap().unwrap();
        assert_eq!(r.key, "primary");
        assert_eq!(r.source, KeySource::Env("TYPESAFE_API_KEY"));

        let r = resolve_with(Jev, |_| None, Some(&path)).unwrap().unwrap();
        assert_eq!(r.key, "file-key-123456");
        assert!(matches!(r.source, KeySource::File(_)));
    }

    #[test]
    fn blank_env_values_are_ignored() {
        let env = |n: &str| (n == "TYPESAFE_API_KEY").then(|| "  ".to_owned());
        assert!(resolve_with(Jev, env, None).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn stored_file_is_private_and_loose_mode_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("heal/credentials.toml");
        store(&path, Jev, "abcdefghijkl").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve_with(Jev, |_| None, Some(&path)).is_err());
    }

    /// A made-up key, assembled at run time so secret scanners that read
    /// the source do not mistake the fixture for a real credential.
    fn fake_key() -> String {
        ["tk", "live", "0123456789abcdef"].join("-")
    }

    #[test]
    fn malformed_file_errors_never_quote_the_key() {
        let fake = fake_key();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.toml");
        for body in [
            format!("[jev]\napi_key = {fake}\n"),
            format!("[jev]\nkey = \"{fake}\"\n"),
            format!("jev = \"{fake}\"\n"),
        ] {
            write_private(&path, body.as_bytes()).unwrap();
            let err = resolve_with(Jev, |_| None, Some(&path)).unwrap_err();
            assert!(!err.contains(&fake), "leaked in: {err}");
            assert!(err.contains("line "), "{err}");
        }
    }

    #[test]
    fn debug_output_masks_the_key() {
        let r = ResolvedKey {
            key: fake_key(),
            source: KeySource::Env("TYPESAFE_API_KEY"),
        };
        let shown = format!("{r:?}");
        assert!(!shown.contains("0123456789"), "{shown}");
        assert!(shown.contains("tk-l…cdef"), "{shown}");
    }

    #[test]
    fn clear_reports_whether_a_key_existed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        assert!(!clear(&path, Jev).unwrap());
        store(&path, Jev, "abcdefghijkl").unwrap();
        assert!(clear(&path, Jev).unwrap());
    }

    #[test]
    fn providers_keep_separate_keys_in_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        store(&path, Jev, "jjjjjjjjjjjj").unwrap();
        store(&path, Claude, "cccccccccccc").unwrap();
        let read = |p| resolve_with(p, |_| None, Some(&path)).unwrap();
        assert_eq!(read(Jev).unwrap().key, "jjjjjjjjjjjj");
        assert_eq!(read(Claude).unwrap().key, "cccccccccccc");

        assert!(clear(&path, Jev).unwrap());
        assert!(read(Jev).is_none());
        assert_eq!(read(Claude).unwrap().key, "cccccccccccc");
        assert!(!clear(&path, Jev).unwrap(), "already gone");
        assert!(path.exists());

        assert!(clear(&path, Claude).unwrap());
        assert!(!path.exists(), "no key left");
    }

    #[test]
    fn claude_reads_only_its_own_variable() {
        let env = |n: &str| (n == "TYPESAFE_API_KEY").then(|| "jev".to_owned());
        assert!(resolve_with(Claude, env, None).unwrap().is_none());
        let env = |n: &str| (n == "ANTHROPIC_API_KEY").then(|| "anthropic".to_owned());
        let r = resolve_with(Claude, env, None).unwrap().unwrap();
        assert_eq!(r.source, KeySource::Env("ANTHROPIC_API_KEY"));
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
