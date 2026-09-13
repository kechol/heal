use std::collections::BTreeMap;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const SOURCE_CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceCache<T> {
    version: u32,
    pub entries: BTreeMap<String, SourceCacheEntry<T>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceCacheEntry<T> {
    pub size: u64,
    pub content_hash: u64,
    pub language: String,
    pub analyzer_version: u32,
    pub capabilities: u8,
    pub data: T,
}

impl<T> Default for SourceCache<T> {
    fn default() -> Self {
        Self {
            version: SOURCE_CACHE_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

impl<T: DeserializeOwned> SourceCache<T> {
    pub fn load(path: &Path) -> Self {
        let Ok(body) = std::fs::read(path) else {
            return Self::default();
        };
        serde_json::from_slice(&body)
            .ok()
            .filter(|cache: &Self| cache.version == SOURCE_CACHE_VERSION)
            .unwrap_or_default()
    }
}

impl<T: Serialize> SourceCache<T> {
    pub fn save(&self, path: &Path) {
        let Some(dir) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let ignore_path = dir.join(".gitignore");
        if !ignore_path.exists() {
            let _ = crate::core::fs::atomic_write(&ignore_path, b"*\n");
        }
        let Ok(ignore_body) = std::fs::read_to_string(&ignore_path) else {
            return;
        };
        let mut ignore = ignore::gitignore::GitignoreBuilder::new(dir);
        for line in ignore_body.lines() {
            if ignore.add_line(None, line).is_err() {
                return;
            }
        }
        let Ok(ignore) = ignore.build() else {
            return;
        };
        if !ignore.matched_path_or_any_parents(path, false).is_ignore() {
            return;
        }
        let Ok(body) = serde_json::to_vec(self) else {
            return;
        };
        let _ = crate::core::fs::atomic_write(path, &body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_or_old_cache_falls_back_to_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("cache/source.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json").unwrap();
        assert!(SourceCache::<String>::load(&path).entries.is_empty());
        std::fs::write(&path, br#"{"version":999,"entries":{}}"#).unwrap();
        assert!(SourceCache::<String>::load(&path).entries.is_empty());
    }

    #[test]
    fn save_is_ignored_and_round_trips() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(".heal/cache/source.json");
        let mut cache = SourceCache::default();
        cache.entries.insert(
            "src/lib.rs".into(),
            SourceCacheEntry {
                size: 3,
                content_hash: 7,
                language: "rust".into(),
                analyzer_version: 1,
                capabilities: 1,
                data: "payload".to_owned(),
            },
        );
        cache.save(&path);
        assert_eq!(SourceCache::<String>::load(&path).entries.len(), 1);
        assert_eq!(
            std::fs::read_to_string(path.parent().unwrap().join(".gitignore")).unwrap(),
            "*\n"
        );
    }

    #[test]
    fn save_does_not_create_an_unignored_cache_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let cache_dir = dir.path().join(".heal/cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join(".gitignore"), "other.json\n").unwrap();
        let path = cache_dir.join("source-v1.json");
        SourceCache::<String>::default().save(&path);
        assert!(!path.exists());
    }
}
