//! Path helpers and the hard denylist.
//!
//! The denylist is the last line of defence: whatever a rule or the user
//! selects, the cleaner refuses anything listed here.

use std::path::{Component, Path, PathBuf};

/// Expands a leading `~` or `~/` against `home`.
pub fn expand_tilde(pattern: &str, home: &Path) -> PathBuf {
    if pattern == "~" {
        home.to_path_buf()
    } else if let Some(rest) = pattern.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(pattern)
    }
}

/// Same as [`expand_tilde`] but as a string, for glob matching.
pub fn expand_tilde_str(pattern: &str, home: &Path) -> String {
    expand_tilde(pattern, home).to_string_lossy().into_owned()
}

/// True if the path is absolute and has no `.`/`..` components, so that a
/// prefix check on it means what it says.
pub fn is_normalized_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_) | Component::Prefix(_)))
}

/// System locations that are never touched, whatever the home folder is.
const SYSTEM_DENY: &[&str] = &[
    "/System",
    "/bin",
    "/sbin",
    "/usr",
    "/etc",
    "/var",
    "/private",
    "/dev",
    "/cores",
    "/opt",
    "/Library",
    "/Applications",
    "/Volumes",
    "/Users/Shared",
];

/// Home-relative folders whose contents are never touched: user data,
/// iCloud, credentials and Apple's own databases.
const HOME_DENY_INSIDE: &[&str] = &[
    "Library/Mobile Documents",
    "Library/CloudStorage",
    "Library/Keychains",
    "Library/Mail",
    "Library/Messages",
    "Library/Photos",
    "Library/Group Containers",
    "Library/Accounts",
    "Library/Cookies",
    "Library/Calendars",
    "Library/Safari",
    "Library/Preferences",
    "Library/IdentityServices",
    "Library/Biome",
    "Library/Daemon Containers",
    ".ssh",
    ".gnupg",
    ".config",
    ".git",
];

/// Home-relative folders that may contain cleanable items but must never be
/// removed themselves.
const HOME_DENY_EXACT: &[&str] = &[
    "",
    "Library",
    "Library/Caches",
    "Library/Application Support",
    "Library/Containers",
    "Library/Developer",
    "Library/Logs",
    "Desktop",
    "Documents",
    "Downloads",
    "Movies",
    "Music",
    "Pictures",
    "Public",
    "Applications",
    ".Trash",
    ".cache",
    ".local",
    ".local/share",
];

/// Returns why `path` must not be deleted, or `None` if the denylist allows it.
///
/// Everything outside `home` is denied: OpenDisk only cleans the user's own
/// files (system-level cleaning goes through official tools such as `tmutil`).
pub fn deny_reason(path: &Path, home: &Path) -> Option<String> {
    if !is_normalized_absolute(path) {
        return Some("path is not absolute and normalized".into());
    }
    let Ok(rel) = path.strip_prefix(home) else {
        for sys in SYSTEM_DENY {
            if path.starts_with(sys) {
                return Some(format!("{sys} is a protected system location"));
            }
        }
        return Some("outside the home folder".into());
    };
    for exact in HOME_DENY_EXACT {
        if rel == Path::new(exact) {
            return Some(format!("~/{exact} itself is never removed"));
        }
    }
    for inside in HOME_DENY_INSIDE {
        if rel.starts_with(inside) {
            return Some(format!("~/{inside} holds personal data or credentials"));
        }
    }
    let comps: Vec<&str> = rel.components().filter_map(|c| c.as_os_str().to_str()).collect();
    if comps.iter().any(|c| c.ends_with(".photoslibrary")) {
        return Some("inside a Photos library".into());
    }
    if comps.len() >= 3 && comps[0] == "Library" && comps[1] == "Containers" && comps[2].starts_with("com.apple.") {
        return Some("Apple app container".into());
    }
    None
}

/// Checks that no ancestor of `path` (below `/`) is a symlink, so the path
/// resolves to where it says it does.
pub fn ancestors_are_real(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return true;
    };
    match std::fs::canonicalize(parent) {
        // On macOS /var and /tmp are symlinks into /private; tests run there.
        Ok(real) => real == parent || strip_private(&real) == parent,
        Err(_) => false,
    }
}

fn strip_private(p: &Path) -> PathBuf {
    match p.strip_prefix("/private") {
        Ok(rest) if cfg!(target_os = "macos") => Path::new("/").join(rest),
        _ => p.to_path_buf(),
    }
}

/// Where OpenDisk keeps its journal and scan index.
pub fn data_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/OpenDisk")
    } else {
        home.join(".local/share/opendisk")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denylist() {
        let home = Path::new("/Users/me");
        assert!(deny_reason(Path::new("/System/Library"), home).is_some());
        assert!(deny_reason(Path::new("/Users/me"), home).is_some());
        assert!(deny_reason(Path::new("/Users/me/Library/Caches"), home).is_some());
        assert!(deny_reason(Path::new("/Users/me/Library/Mail/V10"), home).is_some());
        assert!(deny_reason(Path::new("/Users/me/Library/Mobile Documents/x"), home).is_some());
        assert!(deny_reason(Path::new("/Users/me/Library/Containers/com.apple.Notes/Data"), home).is_some());
        assert!(deny_reason(Path::new("/Users/me/Pictures/Photos Library.photoslibrary/x"), home).is_some());
        assert!(deny_reason(Path::new("/Users/me/Library/Caches/../Mail"), home).is_some());
        assert!(deny_reason(Path::new("/Users/other/Library/Caches/x"), home).is_some());
        assert!(deny_reason(Path::new("/Users/me/Library/Caches/com.foo"), home).is_none());
        assert!(deny_reason(Path::new("/Users/me/.npm/_cacache"), home).is_none());
    }

    #[test]
    fn tilde() {
        let home = Path::new("/h");
        assert_eq!(expand_tilde("~/a/b", home), PathBuf::from("/h/a/b"));
        assert_eq!(expand_tilde("~", home), PathBuf::from("/h"));
        assert_eq!(expand_tilde("/abs", home), PathBuf::from("/abs"));
    }
}
