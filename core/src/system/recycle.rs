//! Pure helpers for the Windows Recycle Bin.
//!
//! Each item in `C:\$Recycle.Bin\<SID>` is a pair: `$R...` holds the file or
//! folder and `$I...` a small record with its original path and deletion time.
//! The format is documented by the forensics community, not by Microsoft, so
//! the parser is defensive and every field is checked.

/// Contents of an `$I` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecycleInfo {
    /// 1 or 2, the only versions Windows has used.
    pub version: u64,
    /// Original size in bytes.
    pub size: u64,
    /// Deletion time as a Windows FILETIME (100 ns since 1601).
    pub deleted_filetime: u64,
    /// Original absolute path.
    pub original: String,
}

fn u16_units(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16(&units).ok()
}

/// Parses an `$I` record. Returns `None` for truncated or unknown versions.
pub fn parse_info(bytes: &[u8]) -> Option<RecycleInfo> {
    if bytes.len() < 24 {
        return None;
    }
    let version = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let size = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let deleted_filetime = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let original = match version {
        1 => {
            // A fixed 260 UTF-16 code units, NUL padded.
            let end = 24 + 520;
            if bytes.len() < end {
                return None;
            }
            u16_units(&bytes[24..end])?
        }
        2 => {
            // A length in code units (including the NUL) followed by UTF-16.
            if bytes.len() < 28 {
                return None;
            }
            let chars = u32::from_le_bytes(bytes[24..28].try_into().ok()?) as usize;
            if chars == 0 || bytes.len() < 28 + chars * 2 {
                return None;
            }
            u16_units(&bytes[28..28 + chars * 2])?
        }
        _ => return None,
    };
    if original.is_empty() {
        return None;
    }
    Some(RecycleInfo {
        version,
        size,
        deleted_filetime,
        original,
    })
}

/// `$Rabc` → `$Iabc`, the record that describes it. `None` for other names.
pub fn sibling_info_name(trashed: &str) -> Option<String> {
    let rest = trashed.strip_prefix("$R")?;
    Some(format!("$I{rest}"))
}

/// Whether `path` is the original path recorded in `info`, ignoring case (and
/// a `\\?\` prefix, which some shell calls add).
pub fn same_original(original: &str, path: &str) -> bool {
    let clean = |s: &str| {
        s.trim_start_matches(r"\\?\")
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    clean(original) == clean(path)
}

/// Recycle Bin limit for one volume, from the per-volume registry values.
///
/// `max_mb` is `MaxCapacity` (megabytes), `nuke` is `NukeOnDelete` (1 means
/// "delete permanently, do not use the Recycle Bin"). Without a key Windows
/// uses about 5% of the volume for the whole bin, so that is the estimate.
pub fn capacity_from_registry(max_mb: Option<u32>, nuke: Option<u32>, volume_total: u64) -> super::TrashCapacity {
    use super::TrashCapacity;
    if nuke == Some(1) || max_mb == Some(0) {
        // 0 MB in the Recycle Bin properties means "do not use the Recycle Bin".
        return TrashCapacity::Disabled;
    }
    if let Some(mb) = max_mb {
        return TrashCapacity::MaxBytes(u64::from(mb) << 20);
    }
    TrashCapacity::MaxBytes(volume_total / 20)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v1(original: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&42u64.to_le_bytes());
        out.extend_from_slice(&1234u64.to_le_bytes());
        let mut units: Vec<u16> = original.encode_utf16().collect();
        units.resize(260, 0);
        for u in units {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    fn v2(original: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&2u64.to_le_bytes());
        out.extend_from_slice(&42u64.to_le_bytes());
        out.extend_from_slice(&1234u64.to_le_bytes());
        let units: Vec<u16> = original.encode_utf16().chain(Some(0)).collect();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for u in units {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    #[test]
    fn parses_both_versions() {
        let a = parse_info(&v1(r"C:\Users\me\a.txt")).unwrap();
        assert_eq!(a.version, 1);
        assert_eq!(a.size, 42);
        assert_eq!(a.deleted_filetime, 1234);
        assert_eq!(a.original, r"C:\Users\me\a.txt");

        let b = parse_info(&v2(r"C:\Users\me\ñandú.txt")).unwrap();
        assert_eq!(b.version, 2);
        assert_eq!(b.original, r"C:\Users\me\ñandú.txt");
    }

    #[test]
    fn rejects_truncated_and_unknown() {
        assert!(parse_info(&[]).is_none());
        assert!(parse_info(&[0u8; 23]).is_none());
        let mut unknown = v2("x");
        unknown[0] = 9;
        assert!(parse_info(&unknown).is_none());
        let mut truncated = v2("a long name");
        truncated.truncate(32);
        assert!(parse_info(&truncated).is_none());
        let mut zero = v2("x");
        zero[24..28].copy_from_slice(&0u32.to_le_bytes());
        assert!(parse_info(&zero).is_none());
    }

    #[test]
    fn names_and_paths() {
        assert_eq!(sibling_info_name("$RABCD.txt").as_deref(), Some("$IABCD.txt"));
        assert_eq!(sibling_info_name("$IABCD.txt"), None);
        assert!(same_original(r"C:\Users\Me\A.txt", r"c:\users\me\a.txt"));
        assert!(same_original(r"\\?\C:\Users\Me\A.txt", r"C:\Users\Me\A.txt"));
        assert!(!same_original(r"C:\Users\me\A.txt", r"C:\Users\me\B.txt"));
    }

    #[test]
    fn capacity() {
        assert_eq!(
            capacity_from_registry(None, Some(1), 100),
            super::super::TrashCapacity::Disabled
        );
        assert_eq!(
            capacity_from_registry(Some(100), None, 0),
            super::super::TrashCapacity::MaxBytes(100 << 20)
        );
        assert_eq!(
            capacity_from_registry(None, None, 1000),
            super::super::TrashCapacity::MaxBytes(50)
        );
    }
}
