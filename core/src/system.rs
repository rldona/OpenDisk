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
        // BSD ps on macOS, procps on Linux; both print the bare command name.
        let out = if cfg!(target_os = "macos") {
            Command::new("/bin/ps").args(["-axco", "comm="]).output()
        } else {
            Command::new("ps").args(["-e", "-o", "comm="]).output()
        };
        let Ok(out) = out else {
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
        "/home/linuxbrew/.linuxbrew/bin",
        "/snap/bin",
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

#[cfg(all(unix, not(target_os = "macos")))]
pub fn total_bytes(path: &Path) -> io::Result<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes())?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: as in `available_bytes`.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return Err(io::Error::last_os_error());
    }
    #[allow(clippy::unnecessary_cast)]
    Ok((st.f_blocks as u64).saturating_mul(st.f_frsize as u64))
}

#[cfg(not(unix))]
pub fn total_bytes(_path: &Path) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "TODO(windows): GetDiskFreeSpaceExW",
    ))
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

/// Moves an item to the home Trash of the freedesktop.org Trash spec
/// (`$XDG_DATA_HOME/Trash`), which GNOME Files, Dolphin and others show and
/// can restore from.
#[cfg(all(unix, not(target_os = "macos")))]
fn trash_impl(path: &Path) -> io::Result<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    trash_into(path, &data_home.join("Trash"))
}

#[cfg(not(unix))]
fn trash_impl(_path: &Path) -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "TODO(windows): IFileOperation with FOF_ALLOWUNDO",
    ))
}

/// Moves `path` into `trash/files` and writes its `trash/info/<name>.trashinfo`
/// record. The info file is created first with `create_new`, which reserves the
/// name (as the spec requires). Only a rename is used, so an item on another
/// volume fails instead of being copied.
#[cfg(unix)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn trash_into(path: &Path, trash: &Path) -> io::Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::DirBuilderExt;

    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy()
        .into_owned();
    let (files, info) = (trash.join("files"), trash.join("info"));
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&files)?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&info)?;
    let record = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode_path(path),
        local_timestamp()
    );
    for n in 0..10_000u32 {
        let candidate = if n == 0 { name.clone() } else { format!("{name}.{n}") };
        let info_path = info.join(format!("{candidate}.trashinfo"));
        let mut f = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let dest = files.join(&candidate);
        if std::fs::symlink_metadata(&dest).is_ok() {
            let _ = std::fs::remove_file(&info_path);
            continue;
        }
        let moved = f
            .write_all(record.as_bytes())
            .and_then(|()| std::fs::rename(path, &dest));
        return match moved {
            Ok(()) => Ok(dest),
            Err(e) => {
                let _ = std::fs::remove_file(&info_path);
                if e.raw_os_error() == Some(libc::EXDEV) {
                    Err(io::Error::other("the item is on a different volume than the Trash"))
                } else {
                    Err(e)
                }
            }
        };
    }
    Err(io::Error::other("could not find a free name in the Trash"))
}

/// After an item is put back from a freedesktop.org Trash, removes its
/// `.trashinfo` record. Does nothing for the macOS Trash.
pub fn forget_trash_info(trashed: &Path) {
    let (Some(name), Some(files)) = (trashed.file_name(), trashed.parent()) else {
        return;
    };
    if files.file_name() != Some(std::ffi::OsStr::new("files")) {
        return;
    }
    let Some(trash) = files.parent() else { return };
    let mut info_name = name.to_os_string();
    info_name.push(".trashinfo");
    let _ = std::fs::remove_file(trash.join("info").join(info_name));
}

/// Percent-encodes a path for the `Path=` key of a `.trashinfo` file.
#[cfg(unix)]
fn percent_encode_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Local time as `YYYY-MM-DDThh:mm:ss`, the `DeletionDate=` format.
#[cfg(unix)]
fn local_timestamp() -> String {
    // SAFETY: `time(NULL)` has no preconditions; `localtime_r` writes into `tm`.
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return "1970-01-01T00:00:00".into();
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
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

    #[test]
    fn freedesktop_trash_moves_item_and_writes_info() {
        let tmp = tempfile::tempdir().unwrap();
        let trash = tmp.path().join("Trash");
        for (i, name) in ["My Cache", "My Cache"].iter().enumerate() {
            let item = tmp.path().join(format!("src{i}")).join(name);
            std::fs::create_dir_all(&item).unwrap();
            std::fs::write(item.join("a.bin"), b"x").unwrap();
            let dest = trash_into(&item, &trash).unwrap();
            assert!(!item.exists());
            assert!(dest.join("a.bin").exists());
            let expected = if i == 0 {
                "My Cache".to_string()
            } else {
                "My Cache.1".to_string()
            };
            assert_eq!(dest, trash.join("files").join(&expected));
            let info = std::fs::read_to_string(trash.join("info").join(format!("{expected}.trashinfo"))).unwrap();
            assert!(info.starts_with("[Trash Info]\nPath=/"));
            assert!(info.contains(&format!("/src{i}/My%20Cache\n")));
            assert!(info.contains("DeletionDate="));
            forget_trash_info(&dest);
            assert!(!trash.join("info").join(format!("{expected}.trashinfo")).exists());
        }
    }
}
