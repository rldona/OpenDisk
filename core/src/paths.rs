//! Path helpers and the hard denylist.
//!
//! The denylist is the last line of defence: whatever a rule or the user
//! selects, the cleaner refuses anything listed here.

use std::path::{Component, Path, PathBuf};

/// How to compare path components for the current platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Sensitive,
    Insensitive,
}

fn same(a: &str, b: &str, case: Case) -> bool {
    match case {
        Case::Sensitive => a == b,
        Case::Insensitive => a.eq_ignore_ascii_case(b),
    }
}

fn starts_with(rel: &[&str], prefix: &str, case: Case) -> bool {
    let want: Vec<&str> = prefix.split('/').filter(|c| !c.is_empty()).collect();
    rel.len() >= want.len() && want.iter().zip(rel).all(|(w, r)| same(w, r, case))
}

/// Expands a leading `~` or `~/` against `home`.
pub fn expand_tilde(pattern: &str, home: &Path) -> PathBuf {
    if pattern == "~" {
        home.to_path_buf()
    } else if let Some(rest) = pattern.strip_prefix("~/") {
        // Join component by component so Windows gets its own separator.
        rest.split('/')
            .filter(|c| !c.is_empty())
            .fold(home.to_path_buf(), |p, c| p.join(c))
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

/// `dunce` behaves like `fs::canonicalize` on Unix and strips the `\\?\`
/// prefix (and normalises short names) on Windows.
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// The user's home folder.
///
/// `$HOME` on Unix, `%USERPROFILE%` (or the known folder) on Windows. `None`
/// means it could not be found, which the CLI and the app must treat as fatal
/// instead of falling back to `/`.
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            if home.is_absolute() && !home.as_os_str().is_empty() {
                return Some(canonicalize(&home).unwrap_or(home));
            }
        }
        crate::system::profile_dir().map(|p| canonicalize(&p).unwrap_or(p))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && !p.as_os_str().is_empty())
    }
}

/// Root of the volume that holds the data to scan.
pub fn data_volume(home: &Path) -> PathBuf {
    let _ = home;
    if cfg!(target_os = "macos") {
        return PathBuf::from("/System/Volumes/Data");
    }
    #[cfg(windows)]
    {
        let mut root = PathBuf::new();
        for c in home.components() {
            match c {
                Component::Prefix(_) | Component::RootDir => root.push(c.as_os_str()),
                _ => break,
            }
        }
        if !root.as_os_str().is_empty() {
            return root;
        }
        PathBuf::from("C:\\")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/")
    }
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

/// Windows top-level folders, used only to explain why something outside the
/// home folder is refused.
const WINDOWS_SYSTEM_DENY: &[&str] = &[
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "$Recycle.Bin",
    "System Volume Information",
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
    ".aws",
    ".azure",
    ".kube",
    ".docker/config.json",
    ".docker/contexts",
    ".local/share/keyrings",
    ".password-store",
    ".pki",
    ".mozilla",
    ".thunderbird",
    "AppData/Local/Packages",
    "AppData/Local/Programs",
    "AppData/Roaming/gnupg",
    "AppData/Local/Microsoft/WindowsApps",
];

/// Folder names that hold rebuildable Chromium/Electron/Windows caches.
const CACHE_DIR_NAMES: &[&str] = &[
    "Cache",
    "Code Cache",
    "GPUCache",
    "CachedData",
    "CachedExtensionVSIXs",
    "CacheStorage",
    "DawnCache",
    "GrShaderCache",
    "ShaderCache",
    "logs",
    "INetCache",
    "cache2",
];

/// Folders whose contents may only be caches: `(prefix, components to skip
/// before the cache folder)`. For example `AppData/Roaming/Code/Cache`: the
/// prefix is `AppData/Roaming`, `Code` is the app and `Cache` the allowed part.
const HOME_CACHE_ONLY: &[(&str, usize)] = &[
    (".config", 1),
    // Longer prefixes first: the first match decides.
    ("AppData/Roaming/Microsoft", 1),
    ("AppData/Local/Microsoft/Edge/User Data", 1),
    ("AppData/Local/Google/Chrome/User Data", 1),
    ("AppData/Local/BraveSoftware/Brave-Browser/User Data", 1),
    ("AppData/Local/Microsoft", 1),
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
    ".local/share/Trash",
    ".local/share/Trash/files",
    ".local/share/Trash/info",
    ".config",
    ".var",
    "snap",
    // Windows user folders.
    "AppData",
    "AppData/Local",
    "AppData/LocalLow",
    "AppData/Roaming",
    "AppData/Roaming/Microsoft",
    "AppData/Local/Temp",
    "AppData/Local/Programs",
    "AppData/Local/Microsoft",
    "AppData/Local/Packages",
    "Videos",
    "Favorites",
    "Links",
    "Contacts",
    "Saved Games",
    "Searches",
    "3D Objects",
];

fn platform_case() -> Case {
    if cfg!(windows) {
        Case::Insensitive
    } else {
        Case::Sensitive
    }
}

/// Rejects components Windows would interpret specially: an alternate data
/// stream (`:`), or a name that ends in a dot or a space (Win32 strips them).
fn suspicious_component(rel: &[&str]) -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    for c in rel {
        if c.contains(':') || c.ends_with('.') || c.ends_with(' ') {
            return Some(format!("~/{c} has a suspicious name"));
        }
    }
    None
}

/// Whether `rel` (components below the home folder) is allowed.
///
/// Pure, so every platform's table can be tested anywhere.
pub fn deny_rel(rel: &[&str], case: Case) -> Option<String> {
    if let Some(reason) = suspicious_component(rel) {
        return Some(reason);
    }
    let joined = rel.join("/");
    for exact in HOME_DENY_EXACT {
        if same(&joined, exact, case) {
            return Some(if exact.is_empty() {
                "the home folder itself is never removed".into()
            } else {
                format!("~/{exact} itself is never removed")
            });
        }
    }
    // Credentials and personal data win over the cache exceptions.
    for inside in HOME_DENY_INSIDE {
        if starts_with(rel, inside, case) {
            return Some(format!("~/{inside} holds personal data or credentials"));
        }
    }
    for (prefix, depth) in HOME_CACHE_ONLY {
        if starts_with(rel, prefix, case) {
            let skip = prefix.split('/').filter(|c| !c.is_empty()).count();
            let rest = &rel[skip..];
            let allowed = rest
                .iter()
                .skip(*depth)
                .any(|c| CACHE_DIR_NAMES.iter().any(|d| same(d, c, case)));
            if !allowed {
                return Some(format!("~/{prefix} holds app data; only its caches are cleaned"));
            }
            return None;
        }
    }
    if starts_with(rel, "Library/Containers", case) {
        if let Some(app) = rel.get(2) {
            if app.to_lowercase().starts_with("com.apple.") {
                return Some("Apple app container".into());
            }
        }
    }
    if let Some(first) = rel.first() {
        if first.to_lowercase().starts_with("onedrive") {
            return Some("inside OneDrive".into());
        }
    }
    for c in rel {
        let lower = c.to_lowercase();
        if lower.starts_with("ntuser.") || lower.starts_with("usrclass.dat") {
            return Some("Windows registry hive".into());
        }
    }
    if rel.iter().any(|c| c.to_lowercase().ends_with(".photoslibrary")) {
        return Some("inside a Photos library".into());
    }
    None
}

/// Components of `path` below `home`, or `None` when it is outside.
fn rel_components<'a>(path: &'a Path, home: &Path, case: Case) -> Option<Vec<&'a str>> {
    let p: Vec<&str> = path.components().filter_map(|c| c.as_os_str().to_str()).collect();
    let h: Vec<&str> = home.components().filter_map(|c| c.as_os_str().to_str()).collect();
    if p.len() < h.len() || !h.iter().zip(&p).all(|(a, b)| same(a, b, case)) {
        return None;
    }
    Some(p[h.len()..].to_vec())
}

/// Returns why `path` must not be deleted, or `None` if the denylist allows it.
///
/// Everything outside `home` is denied: OpenDisk only cleans the user's own
/// files (system-level cleaning goes through official tools such as `tmutil`).
pub fn deny_reason(path: &Path, home: &Path) -> Option<String> {
    if !is_normalized_absolute(path) {
        return Some("path is not absolute and normalized".into());
    }
    let case = platform_case();
    let Some(rel) = rel_components(path, home, case) else {
        for sys in SYSTEM_DENY {
            if path.starts_with(sys) {
                return Some(format!("{sys} is a protected system location"));
            }
        }
        if let Some(first) = path.components().find_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        }) {
            if WINDOWS_SYSTEM_DENY.iter().any(|d| same(d, first, case)) {
                return Some(format!("{first} is a protected system location"));
            }
        }
        return Some("outside the home folder".into());
    };
    deny_rel(&rel, case)
}

/// `~/.config/<app>/.../<cache folder>` and the Windows equivalents.
pub fn is_config_cache(path: &Path) -> bool {
    let rel: Vec<&str> = path.components().filter_map(|c| c.as_os_str().to_str()).collect();
    let case = platform_case();
    HOME_CACHE_ONLY.iter().any(|(prefix, depth)| {
        if !starts_with(&rel, prefix, case) {
            return false;
        }
        let skip = prefix.split('/').filter(|c| !c.is_empty()).count();
        rel.iter()
            .skip(skip + depth)
            .any(|c| CACHE_DIR_NAMES.iter().any(|d| same(d, c, case)))
    })
}

/// Checks that no ancestor of `path` (below the volume root) is a symlink,
/// junction or short name, so the path resolves to where it says it does.
pub fn ancestors_are_real(path: &Path) -> bool {
    #[cfg(windows)]
    {
        // Canonicalising the whole path also resolves junctions and 8.3 names
        // in the last component. A mismatch means something was rewritten.
        let Ok(real) = canonicalize(path) else {
            return false;
        };
        let a = real.to_string_lossy().to_lowercase();
        let b = path.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
        a.trim_end_matches(['\\', '/']) == b
    }
    #[cfg(not(windows))]
    {
        let Some(parent) = path.parent() else {
            return true;
        };
        match canonicalize(parent) {
            // On macOS /var and /tmp are symlinks into /private; tests run there.
            Ok(real) => real == parent || strip_private(&real) == parent,
            Err(_) => false,
        }
    }
}

#[cfg(not(windows))]
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
    } else if cfg!(windows) {
        home.join("AppData/Local/OpenDisk")
    } else {
        home.join(".local/share/opendisk")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(s: &str) -> Vec<&str> {
        s.split('/').collect()
    }

    #[test]
    fn denylist() {
        let home = if cfg!(windows) {
            PathBuf::from(r"C:\Users\me")
        } else {
            PathBuf::from("/Users/me")
        };
        let p = |rel: &str| home.join(rel);
        let outside = if cfg!(windows) {
            PathBuf::from(r"C:\Users\other")
        } else {
            PathBuf::from("/Users/other")
        };
        assert!(deny_reason(Path::new("/System/Library"), &home).is_some());
        assert!(deny_reason(&home, &home).is_some());
        assert!(deny_reason(&p("Library/Caches"), &home).is_some());
        assert!(deny_reason(&p("Library/Mail/V10"), &home).is_some());
        assert!(deny_reason(&p("Library/Mobile Documents/x"), &home).is_some());
        assert!(deny_reason(&p("Library/Containers/com.apple.Notes/Data"), &home).is_some());
        assert!(deny_reason(&p("Pictures/Photos Library.photoslibrary/x"), &home).is_some());
        assert!(deny_reason(&p("Library/Caches/../Mail"), &home).is_some());
        assert!(deny_reason(&outside.join("Library/Caches/x"), &home).is_some());
        assert!(deny_reason(&p("Library/Caches/com.foo"), &home).is_none());
        assert!(deny_reason(&p(".npm/_cacache"), &home).is_none());
        assert!(deny_reason(&p(".ssh/id_ed25519"), &home).is_some());
        assert!(deny_reason(&p(".aws/credentials"), &home).is_some());
    }

    #[test]
    fn windows_denylist_is_case_insensitive() {
        assert!(deny_rel(&parts("appdata/roaming/microsoft/CREDENTIALS/x"), Case::Insensitive).is_some());
        assert!(deny_rel(
            &parts("AppData/Local/Microsoft/Windows/INetCache/IE"),
            Case::Insensitive
        )
        .is_none());
        assert!(deny_rel(
            &parts("AppData/Local/Microsoft/Windows/UsrClass.dat"),
            Case::Insensitive
        )
        .is_some());
        assert!(deny_rel(&parts("AppData/Local/Packages/X/LocalCache"), Case::Insensitive).is_some());
        assert!(deny_rel(&parts("OneDrive - Contoso/a"), Case::Insensitive).is_some());
        assert!(deny_rel(&parts("AppData/Roaming/Code/Cache"), Case::Insensitive).is_none());
        assert!(deny_rel(&parts("AppData/Roaming/Microsoft/Credentials/x"), Case::Insensitive).is_some());
        assert!(deny_rel(&parts("AppData/Roaming/Microsoft/Teams/Cache"), Case::Insensitive).is_none());
        assert!(deny_rel(
            &parts("AppData/Roaming/Apple Computer/MobileSync/Backup/x"),
            Case::Insensitive
        )
        .is_none());
        assert!(deny_rel(&parts("AppData/Local/Temp/x.tmp"), Case::Insensitive).is_none());
        assert!(deny_rel(&parts("AppData/Local"), Case::Insensitive).is_some());
        assert!(deny_rel(
            &parts("AppData/Local/Google/Chrome/User Data/Default/Cache"),
            Case::Insensitive
        )
        .is_none());
        assert!(deny_rel(
            &parts("AppData/Local/Google/Chrome/User Data/Default/Login Data"),
            Case::Insensitive
        )
        .is_some());
        assert!(deny_rel(&parts("Downloads/setup.exe"), Case::Insensitive).is_none());
    }

    #[test]
    fn suspicious_names() {
        if cfg!(windows) {
            assert!(deny_rel(&parts("x:stream"), Case::Insensitive).is_some());
            assert!(deny_rel(&parts("Foo."), Case::Insensitive).is_some());
            assert!(deny_rel(&parts("Foo "), Case::Insensitive).is_some());
        }
    }

    #[test]
    fn config_caches() {
        assert!(is_config_cache(Path::new(".config/Code/Cache")));
        assert!(is_config_cache(Path::new(
            ".config/google-chrome/Default/Service Worker/CacheStorage"
        )));
        assert!(!is_config_cache(Path::new(".config/Code/User/settings.json")));
        assert!(!is_config_cache(Path::new(".config/Cache")));
        let home = if cfg!(windows) {
            PathBuf::from(r"C:\Users\me")
        } else {
            PathBuf::from("/home/me")
        };
        let p = |rel: &str| home.join(rel);
        assert!(deny_reason(&p(".config"), &home).is_some());
        assert!(deny_reason(&p(".config/Code/User"), &home).is_some());
        assert!(deny_reason(&p(".local/share/Trash"), &home).is_some());
        assert!(deny_reason(&p(".local/share/keyrings/login.keyring"), &home).is_some());
        assert!(deny_reason(&p(".config/Code/Cache"), &home).is_none());
        assert!(deny_reason(&p("AppData/Roaming/Microsoft/Credentials/x"), &home).is_some());
        assert!(deny_reason(&p("AppData/Roaming/Microsoft/Teams/Cache"), &home).is_none());
    }

    #[test]
    fn tilde() {
        let home = Path::new("/h");
        assert_eq!(expand_tilde("~/a/b", home), PathBuf::from("/h/a/b"));
        assert_eq!(expand_tilde("~", home), PathBuf::from("/h"));
        assert_eq!(expand_tilde("/abs", home), PathBuf::from("/abs"));
    }

    #[test]
    fn data_volume_is_a_root() {
        let home = if cfg!(windows) {
            PathBuf::from(r"C:\Users\me")
        } else {
            PathBuf::from("/home/me")
        };
        let volume = data_volume(&home);
        assert!(volume.is_absolute(), "{volume:?}");
        if cfg!(target_os = "macos") {
            assert_eq!(volume, PathBuf::from("/System/Volumes/Data"));
        }
    }
}
