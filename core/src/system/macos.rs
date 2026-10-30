//! macOS specifics: APFS `statfs`, the Finder Trash and running apps.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Moves an item to the user's Trash with `-[NSFileManager trashItemAtURL:...]`,
/// the same call Finder uses ("Put Back" works).
pub fn trash_impl(path: &Path) -> io::Result<PathBuf> {
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

/// `lsappinfo list` prints one block per app with `bundleID="..."`.
pub fn running_bundle_ids() -> HashSet<String> {
    let Ok(out) = Command::new("/usr/bin/lsappinfo").arg("list").output() else {
        return HashSet::new();
    };
    parse_lsappinfo(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_lsappinfo(text: &str) -> HashSet<String> {
    text.split("bundleID=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|s| !s.is_empty() && *s != "[ NULL ]")
        .map(str::to_string)
        .collect()
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
        assert!(total_bytes(tmp.path()).unwrap() > 0);
    }
}
