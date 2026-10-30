//! Disk usage and file identity as the filesystem sees them.

use std::fs::Metadata;
use std::path::Path;

/// Stable identity of a file inside one machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    pub volume: u64,
    pub index: u64,
}

/// What the scanner and the cleaner need to know about one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInfo {
    /// Bytes allocated on disk: what deleting the file can give back.
    pub allocated: u64,
    /// `None` when the identity could not be read (only Windows: locked files,
    /// unusual filesystems). Never faked with `(0, 0)`.
    pub id: Option<FileId>,
    /// Number of hardlinks pointing at the file.
    pub nlink: u64,
}

/// `md` must come from `symlink_metadata` or `DirEntry::metadata` of `path`.
///
/// On Unix everything comes from the same `Metadata` (no extra syscall). On
/// Windows a file has to be opened to ask for its allocated size and file id.
#[cfg(unix)]
pub fn info(_path: &Path, md: &Metadata) -> FileInfo {
    use std::os::unix::fs::MetadataExt;
    FileInfo {
        allocated: md.blocks().saturating_mul(512),
        id: Some(FileId {
            volume: md.dev(),
            index: md.ino(),
        }),
        nlink: md.nlink(),
    }
}

#[cfg(windows)]
pub fn info(path: &Path, md: &Metadata) -> FileInfo {
    win::info(path, md)
}

#[cfg(all(not(unix), not(windows)))]
pub fn info(_path: &Path, md: &Metadata) -> FileInfo {
    FileInfo {
        allocated: md.len(),
        id: None,
        nlink: 1,
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::fs::Metadata;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FileIdInfo, FileStandardInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
        BY_HANDLE_FILE_INFORMATION, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_STANDARD_INFO, OPEN_EXISTING,
    };

    use super::{FileId, FileInfo};

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle came from CreateFileW and is closed once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    /// Opens the entry itself (not a reparse target) without reading data, so
    /// OneDrive placeholders are not hydrated.
    fn open(path: &Path) -> windows::core::Result<HANDLE> {
        let name = wide(path);
        // SAFETY: `name` is NUL-terminated and lives for the call.
        unsafe {
            CreateFileW(
                PCWSTR::from_raw(name.as_ptr()),
                FILE_READ_ATTRIBUTES.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0),
                None,
            )
        }
    }

    fn standard(handle: HANDLE) -> Option<FILE_STANDARD_INFO> {
        let mut info = FILE_STANDARD_INFO::default();
        // SAFETY: `info` is writable and its size is passed.
        unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileStandardInfo,
                &mut info as *mut _ as *mut c_void,
                size_of::<FILE_STANDARD_INFO>() as u32,
            )
            .ok()?;
        }
        Some(info)
    }

    fn identity(handle: HANDLE) -> Option<FileId> {
        let mut id = FILE_ID_INFO::default();
        // SAFETY: `id` is writable and its size is passed.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                &mut id as *mut _ as *mut c_void,
                size_of::<FILE_ID_INFO>() as u32,
            )
        }
        .is_ok();
        if ok {
            // NTFS uses the low 64 bits; ReFS may use all 128. Folding the two
            // halves keeps a stable value and collisions stay theoretical.
            let bytes = id.FileId.Identifier;
            let low = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
            let high = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
            return Some(FileId {
                volume: id.VolumeSerialNumber,
                index: low ^ high,
            });
        }
        // FAT/exFAT: the older call still knows the volume and index.
        let mut basic = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `basic` is writable.
        unsafe {
            GetFileInformationByHandle(handle, &mut basic).ok()?;
        }
        Some(FileId {
            volume: u64::from(basic.dwVolumeSerialNumber),
            index: (u64::from(basic.nFileIndexHigh) << 32) | u64::from(basic.nFileIndexLow),
        })
    }

    pub fn info(path: &Path, md: &Metadata) -> FileInfo {
        let mut out = FileInfo {
            allocated: md.len(),
            id: None,
            nlink: 1,
        };
        let Ok(handle) = open(path) else {
            return out;
        };
        let handle = Handle(handle);
        if let Some(std) = standard(handle.0) {
            out.allocated = u64::try_from(std.AllocationSize).unwrap_or(md.len());
            out.nlink = u64::from(std.NumberOfLinks).max(1);
        }
        out.id = identity(handle.0);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocated_is_at_least_the_logical_size() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a.bin");
        std::fs::write(&path, vec![1u8; 1 << 20]).unwrap();
        let md = std::fs::symlink_metadata(&path).unwrap();
        let info = info(&path, &md);
        assert!(
            info.allocated >= 1 << 20,
            "allocated {}, len {}",
            info.allocated,
            md.len()
        );
        assert!(info.id.is_some(), "the identity is known");
        assert_eq!(info.nlink, 1);
    }

    #[test]
    fn hardlinks_share_identity_and_count_links() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.bin");
        let b = tmp.path().join("b.bin");
        std::fs::write(&a, b"x").unwrap();
        std::fs::hard_link(&a, &b).unwrap();
        let ia = info(&a, &std::fs::symlink_metadata(&a).unwrap());
        let ib = info(&b, &std::fs::symlink_metadata(&b).unwrap());
        assert_eq!(ia.nlink, 2);
        assert_eq!(ia.id, ib.id);
    }
}
