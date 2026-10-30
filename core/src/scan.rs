//! Parallel disk walker.
//!
//! Builds a tree of directories (files are folded into their parent's size)
//! with allocated sizes. It never follows symlinks, never crosses into another
//! volume, counts each hardlinked inode once and can be cancelled at any time.
//!
//! TODO(perf): on macOS `getattrlistbulk(2)` returns names, types and sizes of
//! a whole directory in one syscall and is ~2x faster than readdir + lstat.
//! The walk below is a portable rayon-based readdir walk; swap the per-directory
//! listing for getattrlistbulk behind `cfg(target_os = "macos")`.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dashmap::DashSet;
use rayon::prelude::*;
use serde::Serialize;

use crate::size;

/// Live counters a UI can poll while a scan runs.
#[derive(Debug, Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProgressSnapshot {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
}

impl Progress {
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            files: self.files.load(Ordering::Relaxed),
            dirs: self.dirs.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<Progress>,
    /// When set, files at least this big are also returned in
    /// [`ScanResult::large_files`]. `None` keeps the walk lean (measuring a
    /// single folder does not need them).
    pub large_min: Option<u64>,
}

impl ScanOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            cancel: Arc::default(),
            progress: Arc::default(),
            large_min: None,
        }
    }

    /// Also collect the individual files bigger than `min_bytes`.
    pub fn collect_large_files(mut self, min_bytes: u64) -> Self {
        self.large_min = Some(min_bytes);
        self
    }
}

/// A directory and everything below it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DirNode {
    pub name: String,
    /// Allocated bytes of this directory and everything inside it.
    pub size: u64,
    /// Number of files (non-directories) inside, recursively.
    pub files: u64,
    /// Allocated bytes of the files directly inside this directory.
    pub own_files_size: u64,
    /// The directory could not be read (usually missing Full Disk Access).
    pub denied: bool,
    /// Sorted by size, largest first.
    pub children: Vec<DirNode>,
}

impl DirNode {
    /// Finds the node for `rel`, a path relative to this node.
    pub fn find(&self, rel: &Path) -> Option<&DirNode> {
        let mut node = self;
        for comp in rel.components() {
            let Component::Normal(name) = comp else {
                continue;
            };
            let name = name.to_string_lossy();
            node = node.children.iter().find(|c| c.name == name)?;
        }
        Some(node)
    }
}

/// A single file above the collection threshold, largest first.
#[derive(Debug, Clone, Serialize)]
pub struct LargeFile {
    pub path: PathBuf,
    /// Allocated bytes on disk.
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub root: PathBuf,
    pub tree: DirNode,
    pub cancelled: bool,
    pub stats: ProgressSnapshot,
    /// Up to [`MAX_DENIED`] directories that could not be read.
    pub denied: Vec<PathBuf>,
    /// Files above the collection threshold, largest first. Empty unless
    /// [`ScanOptions::collect_large_files`] was used.
    pub large_files: Vec<LargeFile>,
    pub duration_ms: u64,
}

pub const MAX_DENIED: usize = 200;

impl ScanResult {
    /// Looks up a directory by absolute path. Also understands the macOS
    /// firmlinks between `/` and `/System/Volumes/Data` (so a whole-volume scan
    /// can answer questions about `/Users/...`).
    pub fn find(&self, path: &Path) -> Option<&DirNode> {
        if let Ok(rel) = path.strip_prefix(&self.root) {
            return self.tree.find(rel);
        }
        let data = Path::new("/System/Volumes/Data");
        if let Ok(root_rel) = self.root.strip_prefix(data) {
            let root_as_firm = Path::new("/").join(root_rel);
            if let Ok(rel) = path.strip_prefix(&root_as_firm) {
                return self.tree.find(rel);
            }
        }
        None
    }
}

fn mtime_secs(md: &fs::Metadata) -> Option<u64> {
    md.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

struct Walker<'a> {
    root_volume: Option<u64>,
    seen_inodes: DashSet<size::FileId>,
    cancel: &'a AtomicBool,
    progress: &'a Progress,
    denied: Mutex<Vec<PathBuf>>,
    large_min: Option<u64>,
    large: Mutex<Vec<LargeFile>>,
}

impl Walker<'_> {
    fn note_error(&self, path: &Path, err: &io::Error) {
        self.progress.errors.fetch_add(1, Ordering::Relaxed);
        if err.kind() == io::ErrorKind::PermissionDenied {
            let mut denied = self.denied.lock().unwrap();
            if denied.len() < MAX_DENIED {
                denied.push(path.to_path_buf());
            }
        }
    }

    /// Size a file counts for: zero if this inode was already counted.
    fn file_size(&self, path: &Path, md: &fs::Metadata) -> u64 {
        let info = size::info(path, md);
        if info.nlink > 1 {
            if let Some(id) = info.id {
                if !self.seen_inodes.insert(id) {
                    return 0;
                }
            }
        }
        info.allocated
    }

    fn walk(&self, path: &Path, name: String, own_size: u64) -> DirNode {
        let mut node = DirNode {
            name,
            size: own_size,
            ..Default::default()
        };
        if self.cancel.load(Ordering::Relaxed) {
            return node;
        }
        let entries = match fs::read_dir(path) {
            Ok(e) => e,
            Err(err) => {
                node.denied = err.kind() == io::ErrorKind::PermissionDenied;
                self.note_error(path, &err);
                return node;
            }
        };
        let mut subdirs: Vec<(PathBuf, String, u64)> = Vec::new();
        let mut files = 0u64;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    self.note_error(path, &err);
                    continue;
                }
            };
            // DirEntry::metadata does not follow symlinks (lstat semantics).
            let md = match entry.metadata() {
                Ok(md) => md,
                Err(err) => {
                    self.note_error(&entry.path(), &err);
                    continue;
                }
            };
            if md.is_dir() {
                let info = size::info(&entry.path(), &md);
                if let (Some(root), Some(id)) = (self.root_volume, info.id) {
                    if id.volume != root {
                        continue; // mount point of another volume: don't cross
                    }
                }
                let child_name = entry.file_name().to_string_lossy().into_owned();
                subdirs.push((entry.path(), child_name, info.allocated));
            } else {
                // Regular files, symlinks (the link itself), sockets, ...
                let size = self.file_size(&entry.path(), &md);
                if let Some(min) = self.large_min {
                    // A hardlink already counted elsewhere returns 0, so the
                    // list does not offer the same inode twice.
                    if size > 0 && size >= min {
                        self.large.lock().unwrap().push(LargeFile {
                            // On macOS the data volume is a firmlink; list the
                            // path the user knows so home checks and Trash work.
                            path: crate::paths::logical_path(&entry.path()),
                            size,
                            modified: mtime_secs(&md),
                        });
                    }
                }
                node.own_files_size += size;
                files += 1;
            }
        }
        self.progress.files.fetch_add(files, Ordering::Relaxed);
        self.progress.dirs.fetch_add(1, Ordering::Relaxed);
        self.progress
            .bytes
            .fetch_add(node.own_files_size + own_size, Ordering::Relaxed);

        let mut children: Vec<DirNode> = if subdirs.len() > 1 {
            subdirs.into_par_iter().map(|(p, n, s)| self.walk(&p, n, s)).collect()
        } else {
            subdirs.into_iter().map(|(p, n, s)| self.walk(&p, n, s)).collect()
        };
        children.sort_unstable_by_key(|c| std::cmp::Reverse(c.size));
        node.files = files + children.iter().map(|c| c.files).sum::<u64>();
        node.size += node.own_files_size + children.iter().map(|c| c.size).sum::<u64>();
        node.children = children;
        node
    }
}

fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism().map_or(8, |n| n.get()).clamp(4, 16);
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            // Recursion depth follows directory depth (deep node_modules).
            .stack_size(16 << 20)
            .thread_name(|i| format!("opendisk-scan-{i}"))
            .build()
            .expect("scan thread pool")
    })
}

/// Scans `opts.root`. Returns an error only if the root itself can't be read.
pub fn scan(opts: &ScanOptions) -> io::Result<ScanResult> {
    let start = Instant::now();
    let root_md = fs::symlink_metadata(&opts.root)?;
    if !root_md.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "scan root is not a directory",
        ));
    }
    let walker = Walker {
        root_volume: size::info(&opts.root, &root_md).id.map(|id| id.volume),
        seen_inodes: DashSet::new(),
        cancel: &opts.cancel,
        progress: &opts.progress,
        denied: Mutex::default(),
        large_min: opts.large_min,
        large: Mutex::default(),
    };
    let name = opts.root.to_string_lossy().into_owned();
    let root_size = size::info(&opts.root, &root_md).allocated;
    let tree = pool().install(|| walker.walk(&opts.root, name, root_size));
    let mut large_files = walker.large.into_inner().unwrap();
    large_files.sort_unstable_by_key(|f| std::cmp::Reverse(f.size));
    Ok(ScanResult {
        root: opts.root.clone(),
        tree,
        cancelled: opts.cancel.load(Ordering::Relaxed),
        stats: opts.progress.snapshot(),
        denied: walker.denied.into_inner().unwrap(),
        large_files,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

/// Allocated size and file count of a single path (file or directory), with
/// the same rules as [`scan`].
pub fn measure(path: &Path) -> io::Result<(u64, u64)> {
    let md = fs::symlink_metadata(path)?;
    if !md.is_dir() {
        return Ok((size::info(path, &md).allocated, 1));
    }
    let res = scan(&ScanOptions::new(path))?;
    Ok((res.tree.size, res.tree.files))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(path: &Path, len: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&vec![7u8; len]).unwrap();
    }

    /// Directory link: a symlink on Unix, a junction on Windows. Junctions do
    /// not need developer mode or administrator rights.
    fn link_dir(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        {
            // mklink treats `/` as a switch, so use native separators.
            let native = |p: &Path| p.to_string_lossy().replace('/', "\\");
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(native(link))
                .arg(native(target))
                .status()
                .unwrap();
            assert!(status.success(), "mklink /J {link:?} -> {target:?}");
        }
    }

    #[test]
    fn sizes_hardlinks_and_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("a/big.bin"), 1 << 20);
        write(&root.join("b/small.txt"), 10);
        fs::hard_link(root.join("a/big.bin"), root.join("b/big-link.bin")).unwrap();
        // A link to a big directory elsewhere must not be followed.
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("huge.bin"), 4 << 20);
        link_dir(outside.path(), &root.join("link-to-outside"));

        let res = scan(&ScanOptions::new(root)).unwrap();
        let total = res.tree.size;
        assert!(total >= 1 << 20, "counts the big file");
        assert!(
            total < 2 << 20,
            "hardlink counted once and symlink not followed: {total}"
        );
        assert_eq!(res.tree.files, 4, "3 regular entries + the symlink itself");
        assert!(!res.cancelled);
        // The shared inode counts under whichever folder the walk reached first,
        // which depends on the file system's directory order.
        let a = res.tree.find(Path::new("a")).unwrap();
        let b = res.tree.find(Path::new("b")).unwrap();
        assert!(a.size.max(b.size) >= 1 << 20);
        assert!(
            res.tree.children.windows(2).all(|w| w[0].size >= w[1].size),
            "sorted by size"
        );
    }

    #[test]
    fn collects_large_files_once_and_skips_links() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("big.bin"), 2 << 20);
        write(&root.join("small.txt"), 10);
        fs::hard_link(root.join("big.bin"), root.join("big-link.bin")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("outside.bin"), 4 << 20);
        link_dir(outside.path(), &root.join("link-out"));

        let res = scan(&ScanOptions::new(root).collect_large_files(1 << 20)).unwrap();
        assert_eq!(res.large_files.len(), 1, "{:?}", res.large_files);
        let file = &res.large_files[0];
        assert!(file.path.ends_with("big.bin") || file.path.ends_with("big-link.bin"));
        assert!(file.size >= 2 << 20);
        assert!(file.modified.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn allocated_not_logical() {
        let tmp = tempfile::tempdir().unwrap();
        let f = fs::File::create(tmp.path().join("sparse")).unwrap();
        f.set_len(1 << 30).unwrap(); // 1 GiB logical, nothing allocated
        let (bytes, files) = measure(&tmp.path().join("sparse")).unwrap();
        assert_eq!(files, 1);
        assert!(bytes < 1 << 20, "sparse file reports allocated size, got {bytes}");
    }

    #[test]
    fn cancel_stops_early() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..20 {
            write(&tmp.path().join(format!("d{i}/f")), 100);
        }
        let opts = ScanOptions::new(tmp.path());
        opts.cancel.store(true, Ordering::Relaxed);
        let res = scan(&opts).unwrap();
        assert!(res.cancelled);
        assert!(res.tree.children.is_empty());
    }

    #[test]
    fn firmlink_lookup() {
        let res = ScanResult {
            root: "/System/Volumes/Data".into(),
            tree: DirNode {
                name: "/".into(),
                children: vec![DirNode {
                    name: "Users".into(),
                    size: 5,
                    ..Default::default()
                }],
                ..Default::default()
            },
            cancelled: false,
            stats: Progress::default().snapshot(),
            denied: vec![],
            large_files: vec![],
            duration_ms: 0,
        };
        assert_eq!(res.find(Path::new("/Users")).unwrap().size, 5);
        assert_eq!(res.find(Path::new("/System/Volumes/Data/Users")).unwrap().size, 5);
    }
}
