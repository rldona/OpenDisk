//! Disk usage as the filesystem sees it.

use std::fs::Metadata;

/// Bytes actually allocated on disk for `md` (`st_blocks * 512`).
///
/// This is what `du` reports and what deleting the file can give back, unlike
/// the logical length: sparse files (e.g. `Docker.raw`) use far less, and
/// small files use a whole block. APFS clones share blocks, so freeing a clone
/// may give back less than this; the cleaner reports the real `statfs` delta.
#[cfg(unix)]
pub fn allocated(md: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    md.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
pub fn allocated(md: &Metadata) -> u64 {
    // TODO(windows): use GetCompressedFileSizeW for the allocated size.
    md.len()
}

/// `(device, inode)` of a file, used to count hardlinks once and to detect
/// volume crossings.
#[cfg(unix)]
pub fn dev_ino(md: &Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (md.dev(), md.ino())
}

#[cfg(not(unix))]
pub fn dev_ino(_md: &Metadata) -> (u64, u64) {
    (0, 0)
}

/// Number of hardlinks pointing at the inode.
#[cfg(unix)]
pub fn nlink(md: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    md.nlink()
}

#[cfg(not(unix))]
pub fn nlink(_md: &Metadata) -> u64 {
    1
}
