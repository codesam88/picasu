use std::path::{Path, PathBuf};

use crate::model::config::APP_CONFIG;

/// Resolve a namespace + relative path to a full filesystem path.
///
/// Returns `None` if the namespace is not configured.
#[allow(dead_code)]
pub fn namespace_resolve(namespace: &str, relative: &str) -> Option<PathBuf> {
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("RwLock poisoned");
    config
        .namespaces
        .iter()
        .find(|ns| ns.name == namespace)
        .map(|ns| ns.path.join(relative))
}

/// Reverse-resolve an absolute path to `(namespace, relative)`.
///
/// Uses longest-prefix matching so a trash root nested under a shared root
/// resolves correctly. Returns `None` if the path is not under any namespace.
#[allow(dead_code)]
pub fn namespace_from_path(absolute: &Path) -> Option<(String, String)> {
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("RwLock poisoned");

    let mut best: Option<(String, String)> = None;
    let mut best_len = 0;

    for ns in &config.namespaces {
        if let Ok(relative) = absolute.strip_prefix(&ns.path) {
            let relative_str = relative.to_string_lossy().to_string();
            // Longest-prefix match: prefer the namespace whose root is deepest
            if ns.path.components().count() > best_len {
                best_len = ns.path.components().count();
                best = Some((ns.name.clone(), relative_str));
            }
        }
    }

    best
}

/// Get the root directory for a namespace.
///
/// Returns `None` if the namespace is not configured.
#[allow(dead_code)]
pub fn namespace_root(namespace: &str) -> Option<PathBuf> {
    let config = APP_CONFIG
        .get()
        .expect("APP_CONFIG not initialized")
        .read()
        .expect("RwLock poisoned");
    config
        .namespaces
        .iter()
        .find(|ns| ns.name == namespace)
        .map(|ns| ns.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::config::{APP_CONFIG, AppConfig, NamespaceConfig};
    use std::sync::{Mutex, OnceLock, RwLock};

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    /// Set up namespaces and return a guard that must be held for the
    /// duration of the test to prevent parallel tests from clobbering
    /// the global APP_CONFIG.
    fn setup(namespaces: Vec<NamespaceConfig>) -> std::sync::MutexGuard<'static, ()> {
        let lock = TEST_LOCK.get_or_init(|| Mutex::new(()));
        let guard = lock.lock().unwrap();
        let _ = APP_CONFIG.set(RwLock::new(AppConfig {
            namespaces: namespaces.clone(),
            ..AppConfig::default()
        }));
        if let Some(lock) = APP_CONFIG.get() {
            let mut cfg = lock.write().expect("lock poisoned");
            cfg.namespaces = namespaces;
        }
        guard
    }

    #[test]
    fn resolve_returns_full_path() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = setup(vec![NamespaceConfig {
            name: "shared".to_string(),
            path: dir.path().to_path_buf(),
        }]);
        let result = namespace_resolve("shared", "photo.jpg").unwrap();
        assert_eq!(result, dir.path().join("photo.jpg"));
    }

    #[test]
    fn resolve_unknown_namespace_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = setup(vec![NamespaceConfig {
            name: "shared".to_string(),
            path: dir.path().to_path_buf(),
        }]);
        assert!(namespace_resolve("missing", "photo.jpg").is_none());
    }

    #[test]
    fn from_path_returns_namespace_and_relative() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = setup(vec![NamespaceConfig {
            name: "shared".to_string(),
            path: dir.path().to_path_buf(),
        }]);
        let abs = dir.path().join("sub").join("photo.jpg");
        let (ns, rel) = namespace_from_path(&abs).unwrap();
        assert_eq!(ns, "shared");
        assert_eq!(rel, "sub/photo.jpg");
    }

    #[test]
    fn from_path_longest_prefix_wins() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared");
        let trash = shared.join("trash");
        std::fs::create_dir_all(&trash).unwrap();
        let _guard = setup(vec![
            NamespaceConfig {
                name: "shared".to_string(),
                path: shared.clone(),
            },
            NamespaceConfig {
                name: "trash".to_string(),
                path: trash.clone(),
            },
        ]);
        let abs = trash.join("photo.jpg");
        let (ns, _) = namespace_from_path(&abs).unwrap();
        assert_eq!(ns, "trash", "longest-prefix should pick trash over shared");
    }

    #[test]
    fn from_path_outside_namespaces_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = setup(vec![NamespaceConfig {
            name: "shared".to_string(),
            path: dir.path().to_path_buf(),
        }]);
        let abs = PathBuf::from("/completely/unrelated/path.jpg");
        assert!(namespace_from_path(&abs).is_none());
    }

    #[test]
    fn root_returns_path() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = setup(vec![NamespaceConfig {
            name: "shared".to_string(),
            path: dir.path().to_path_buf(),
        }]);
        assert_eq!(namespace_root("shared").unwrap(), dir.path());
        assert!(namespace_root("missing").is_none());
    }

    #[test]
    fn round_trip_resolve_from_path() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = setup(vec![NamespaceConfig {
            name: "shared".to_string(),
            path: dir.path().to_path_buf(),
        }]);
        let resolved = namespace_resolve("shared", "a/b/photo.jpg").unwrap();
        let (ns, rel) = namespace_from_path(&resolved).unwrap();
        assert_eq!(ns, "shared");
        assert_eq!(rel, "a/b/photo.jpg");
    }
}
