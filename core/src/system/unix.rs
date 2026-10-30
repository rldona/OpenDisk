//! freedesktop.org platforms: `statvfs` and the home Trash.

use std::io;
use std::path::{Path, PathBuf};

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

/// Moves an item to the home Trash of the freedesktop.org Trash spec
/// (`$XDG_DATA_HOME/Trash`), which GNOME Files, Dolphin and others show and
/// can restore from.
pub fn trash_impl(path: &Path) -> io::Result<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    trash_into(path, &data_home.join("Trash"))
}

/// Moves `path` into `trash/files` and writes its `trash/info/<name>.trashinfo`
/// record. The info file is created first with `create_new`, which reserves the
/// name (as the spec requires). Only a rename is used, so an item on another
/// volume fails instead of being copied.
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
/// `.trashinfo` record.
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
    fn statvfs_works_on_temp() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(available_bytes(tmp.path()).unwrap() > 0);
        assert!(total_bytes(tmp.path()).unwrap() > 0);
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
