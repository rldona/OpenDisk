//! Everything the cleaner needs from the operating system, behind a trait so
//! tests never touch the real Trash, real apps or real commands.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub trait System: Send + Sync {
    /// Moves `path` to the Trash and returns where it ended up (for undo).
    fn trash(&self, path: &Path) -> io::Result<PathBuf>;
    /// Bundle IDs of running GUI apps.
    fn running_bundle_ids(&self) -> HashSet<String>;
    /// Names of running processes (`ps -c` style, no path).
    fn running_processes(&self) -> HashSet<String>;
    /// Bytes available to the user on the volume holding `path`.
    fn available_bytes(&self, path: &Path) -> io::Result<u64>;
    /// Whether `program` is found on `PATH`.
    fn command_exists(&self, program: &str) -> bool;
    /// Runs a command without a shell, returning (success, combined output).
    fn run(&self, argv: &[String]) -> io::Result<(bool, String)>;
}

/// The real operating system.
pub struct RealSystem;

impl System for RealSystem {
    fn trash(&self, path: &Path) -> io::Result<PathBuf> {
        trash_impl(path)
    }

    fn running_bundle_ids(&self) -> HashSet<String> {
        // `lsappinfo list` prints one block per app with `bundleID="..."`.
        let Ok(out) = Command::new("/usr/bin/lsappinfo").arg("list").output() else {
            return HashSet::new();
        };
        parse_lsappinfo(&String::from_utf8_lossy(&out.stdout))
    }

    fn running_processes(&self) -> HashSet<String> {
        let Ok(out) = Command::new("/bin/ps").args(["-axco", "comm="]).output() else {
            return HashSet::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    fn available_bytes(&self, path: &Path) -> io::Result<u64> {
        available_bytes(path)
    }

    fn command_exists(&self, program: &str) -> bool {
        which(program).is_some()
    }

    fn run(&self, argv: &[String]) -> io::Result<(bool, String)> {
        let (prog, args) = argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty command"))?;
        let exe = which(prog).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{prog} not found")))?;
        let out = Command::new(exe).args(args).output()?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.success(), text))
    }
}

pub fn parse_lsappinfo(text: &str) -> HashSet<String> {
    text.split("bundleID=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|s| !s.is_empty() && *s != "[ NULL ]")
        .map(str::to_string)
        .collect()
}

/// Finds `program` on `PATH`, plus the usual Homebrew and developer locations
/// (a GUI app launched from Finder gets a minimal `PATH`).
pub fn which(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return None; // rules name programs, never paths
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for extra in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/Applications/Docker.app/Contents/Resources/bin",
    ] {
        dirs.push(PathBuf::from(extra));
    }
    if let Some(home) = std::env::var_os("HOME") {
        for sub in [
            ".cargo/bin",
            "go/bin",
            ".local/bin",
            "miniconda3/bin",
            "miniforge3/bin",
            "anaconda3/bin",
            "Library/pnpm",
        ] {
            dirs.push(Path::new(&home).join(sub));
        }
    }
    dirs.into_iter().map(|d| d.join(program)).find(|p| p.is_file())
}

/// Bytes available to unprivileged users on the volume holding `path`.
#[cfg(target_os = "macos")]
pub fn available_bytes(path: &Path) -> io::Result<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes())?;
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a writable statfs.
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(st.f_bavail.saturating_mul(u64::from(st.f_bsize)))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn available_bytes(path: &Path) -> io::Result<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes())?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a writable statvfs.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    #[allow(clippy::unnecessary_cast)]
    Ok((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

#[cfg(not(unix))]
pub fn available_bytes(_path: &Path) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "TODO(windows): GetDiskFreeSpaceExW",
    ))
}

/// Total size of the volume holding `path`.
#[cfg(target_os = "macos")]
pub fn total_bytes(path: &Path) -> io::Result<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes())?;
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: as in `available_bytes`.
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(st.f_blocks.saturating_mul(u64::from(st.f_bsize)))
}

#[cfg(not(target_os = "macos"))]
pub fn total_bytes(_path: &Path) -> io::Result<u64> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "TODO: statvfs f_blocks"))
}

/// Moves an item to the user's Trash with `-[NSFileManager trashItemAtURL:...]`,
/// the same call Finder uses ("Put Back" works).
#[cfg(target_os = "macos")]
fn trash_impl(path: &Path) -> io::Result<PathBuf> {
    use objc2::rc::Retained;
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let path_str = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))?;
    let ns_path = NSString::from_str(path_str);
    let url = NSURL::fileURLWithPath(&ns_path);
    let manager = NSFileManager::defaultManager();
    let mut resulting: Option<Retained<NSURL>> = None;
    manager
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|err| io::Error::other(err.localizedDescription().to_string()))?;
    let trashed = resulting
        .and_then(|u| u.path())
        .map(|p| PathBuf::from(p.to_string()))
        .ok_or_else(|| io::Error::other("Trash did not report where the item went"))?;
    Ok(trashed)
}

#[cfg(not(target_os = "macos"))]
fn trash_impl(_path: &Path) -> io::Result<PathBuf> {
    // TODO(linux/windows): freedesktop.org Trash spec / IFileOperation.
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "moving to Trash is only implemented on macOS",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lsappinfo() {
        let sample = r#"
 1) "Finder" ASN:0x0-0x1001:
    bundleID="com.apple.finder"
 2) "Slack" ASN:0x0-0x2002:
    bundleID="com.tinyspeck.slackmacgap"
 3) "helper" ASN:0x0-0x3003:
    bundleID=[ NULL ]
"#;
        let ids = parse_lsappinfo(sample);
        assert!(ids.contains("com.apple.finder"));
        assert!(ids.contains("com.tinyspeck.slackmacgap"));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn statfs_works_on_temp() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(available_bytes(tmp.path()).unwrap() > 0);
    }

    #[test]
    fn which_refuses_paths() {
        assert!(which("/bin/rm").is_none());
        assert!(which("ls").is_some());
    }
}
