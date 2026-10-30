//! Everything the cleaner needs from the operating system, behind a trait so
//! tests never touch the real Trash, real apps or real commands.

pub mod recycle;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(unix, not(target_os = "macos")))]
mod unix;
#[cfg(windows)]
mod windows;

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What the system did with an item somebody asked to recycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashOutcome {
    /// It is in the Trash. `None` means it is there but the exact name is not
    /// known, so Undo cannot restore it automatically.
    Moved(Option<PathBuf>),
    /// The system deleted it permanently instead of moving it (Windows does
    /// this when the item does not fit in the Recycle Bin).
    Erased,
}

/// How much the Trash can hold for one volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashCapacity {
    /// No per-item limit worth blocking on (macOS, freedesktop.org).
    Unlimited,
    /// Windows: an item larger than this may be erased instead of recycled.
    MaxBytes(u64),
    /// Windows: the Recycle Bin is disabled for this volume.
    Disabled,
    /// The limit could not be read; do not move anything there.
    Unknown(String),
}

/// Name of the Trash in the user's language-independent form used by the UI.
pub const TRASH_NAME: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

pub trait System: Send + Sync {
    /// Moves `path` to the Trash.
    fn trash(&self, path: &Path) -> io::Result<TrashOutcome>;
    /// Per-item capacity of the Trash on the volume holding `path`.
    fn trash_capacity(&self, _path: &Path) -> TrashCapacity {
        TrashCapacity::Unlimited
    }
    /// Bundle IDs of running GUI apps (macOS only).
    fn running_bundle_ids(&self) -> HashSet<String> {
        HashSet::new()
    }
    /// Names of running processes, lowercase and without `.exe` on Windows.
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
    fn trash(&self, path: &Path) -> io::Result<TrashOutcome> {
        platform_trash(path)
    }

    fn trash_capacity(&self, path: &Path) -> TrashCapacity {
        platform_trash_capacity(path)
    }

    fn running_bundle_ids(&self) -> HashSet<String> {
        #[cfg(target_os = "macos")]
        {
            macos::running_bundle_ids()
        }
        #[cfg(not(target_os = "macos"))]
        {
            HashSet::new()
        }
    }

    fn running_processes(&self) -> HashSet<String> {
        #[cfg(windows)]
        {
            windows::running_processes()
        }
        #[cfg(not(windows))]
        {
            // BSD ps on macOS, procps on Linux; both print the bare name.
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
        let mut cmd = Command::new(exe);
        cmd.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output()?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok((out.status.success(), text))
    }
}

#[cfg(target_os = "macos")]
fn platform_trash(path: &Path) -> io::Result<TrashOutcome> {
    macos::trash_impl(path).map(|p| TrashOutcome::Moved(Some(p)))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_trash(path: &Path) -> io::Result<TrashOutcome> {
    unix::trash_impl(path).map(|p| TrashOutcome::Moved(Some(p)))
}

#[cfg(windows)]
fn platform_trash(path: &Path) -> io::Result<TrashOutcome> {
    windows::trash_impl(path)
}

#[cfg(not(windows))]
fn platform_trash_capacity(_path: &Path) -> TrashCapacity {
    TrashCapacity::Unlimited
}

#[cfg(windows)]
fn platform_trash_capacity(path: &Path) -> TrashCapacity {
    windows::trash_capacity(path)
}

/// Restores an item the journal moved to the Trash.
pub fn restore_trashed(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::restore_trashed(from, to)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to)?;
        forget_trash_info(from);
        Ok(())
    }
}

/// After an item is put back from the Trash, removes its metadata record.
/// Does nothing on macOS, where the system owns it.
pub fn forget_trash_info(trashed: &Path) {
    #[cfg(windows)]
    {
        windows::forget_trash_info(trashed);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        unix::forget_trash_info(trashed);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = trashed;
    }
}

/// Bytes available to unprivileged users on the volume holding `path`.
#[cfg(target_os = "macos")]
pub fn available_bytes(path: &Path) -> io::Result<u64> {
    macos::available_bytes(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn available_bytes(path: &Path) -> io::Result<u64> {
    unix::available_bytes(path)
}

#[cfg(windows)]
pub fn available_bytes(path: &Path) -> io::Result<u64> {
    windows::available_bytes(path)
}

/// Total size of the volume holding `path`.
#[cfg(target_os = "macos")]
pub fn total_bytes(path: &Path) -> io::Result<u64> {
    macos::total_bytes(path)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn total_bytes(path: &Path) -> io::Result<u64> {
    unix::total_bytes(path)
}

#[cfg(windows)]
pub fn total_bytes(path: &Path) -> io::Result<u64> {
    windows::total_bytes(path)
}

/// The user's profile folder, when the environment does not say (Windows).
pub fn profile_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows::profile_dir()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// `.COM`, `.EXE`, `.BAT` and `.CMD` from `PATHEXT`: the only things
/// `std::process::Command` can run. Empty on Unix.
pub fn executable_exts() -> Vec<String> {
    #[cfg(windows)]
    {
        let allowed = [".com", ".exe", ".bat", ".cmd"];
        let from_env = std::env::var("PATHEXT").unwrap_or_default();
        let mut out: Vec<String> = from_env
            .split(';')
            .map(|e| e.trim().to_ascii_lowercase())
            .filter(|e| allowed.contains(&e.as_str()))
            .collect();
        out.dedup();
        if out.is_empty() {
            out = allowed.iter().map(|e| e.to_string()).collect();
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Directories searched for programs: `PATH` plus the usual install locations.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    #[cfg(windows)]
    {
        for key in ["ProgramFiles", "ProgramFiles(x86)", "ProgramData"] {
            if let Some(base) = std::env::var_os(key).map(PathBuf::from) {
                match key {
                    "ProgramFiles" => {
                        dirs.push(base.join("nodejs"));
                        dirs.push(base.join("Docker/Docker/resources/bin"));
                    }
                    "ProgramData" => dirs.push(base.join("chocolatey/bin")),
                    _ => {}
                }
            }
        }
        for key in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(key).map(PathBuf::from) {
                if key == "APPDATA" {
                    dirs.push(base.join("npm"));
                } else {
                    dirs.push(base.join("pnpm"));
                    dirs.push(base.join("Programs/Python/Python313/Scripts"));
                }
            }
        }
        if let Some(home) = crate::paths::home_dir() {
            for sub in [".cargo/bin", "go/bin", "scoop/shims"] {
                dirs.push(home.join(sub));
            }
        }
    }
    #[cfg(not(windows))]
    {
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
        if let Some(home) = crate::paths::home_dir() {
            for sub in [
                ".cargo/bin",
                "go/bin",
                ".local/bin",
                "miniconda3/bin",
                "miniforge3/bin",
                "anaconda3/bin",
                "Library/pnpm",
            ] {
                dirs.push(home.join(sub));
            }
        }
    }
    // The Store aliases start installers; never run one while cleaning.
    dirs.retain(|d| !d.ends_with("WindowsApps"));
    dirs
}

/// Pure candidate list for [`which`], testable on any platform.
pub fn which_candidates(program: &str, dirs: &[PathBuf], exts: &[String]) -> Vec<PathBuf> {
    if program.is_empty()
        || program == "."
        || program == ".."
        || program.contains('/')
        || program.contains('\\')
        || program.contains(':')
    {
        return Vec::new();
    }
    let has_ext = exts
        .iter()
        .any(|e| program.to_ascii_lowercase().ends_with(&e.to_ascii_lowercase()));
    let mut out = Vec::new();
    for dir in dirs {
        if !dir.is_absolute() {
            continue; // never trust a relative entry in PATH
        }
        if exts.is_empty() || has_ext {
            out.push(dir.join(program));
        } else {
            for ext in exts {
                out.push(dir.join(format!("{program}{ext}")));
            }
        }
    }
    out
}

/// Finds `program` on `PATH`, plus the usual install locations.
pub fn which(program: &str) -> Option<PathBuf> {
    which_candidates(program, &search_dirs(), &executable_exts())
        .into_iter()
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_refuses_paths() {
        assert!(which("/bin/rm").is_none());
        assert!(which("..\\rm").is_none());
        assert!(which("rm:ads").is_none());
    }

    #[test]
    fn which_candidates_respects_pathext() {
        let base = if cfg!(windows) {
            PathBuf::from(r"C:\tools")
        } else {
            PathBuf::from("/usr/bin")
        };
        let dirs = vec![base.clone(), PathBuf::from("relative/bin")];
        assert_eq!(which_candidates("npm", &dirs, &[]), vec![base.join("npm")]);
        let exts = vec![
            ".com".to_string(),
            ".exe".to_string(),
            ".bat".to_string(),
            ".cmd".to_string(),
        ];
        assert_eq!(
            which_candidates("npm", &dirs, &exts),
            vec![
                base.join("npm.com"),
                base.join("npm.exe"),
                base.join("npm.bat"),
                base.join("npm.cmd"),
            ]
        );
        assert_eq!(which_candidates("npm.cmd", &dirs, &exts), vec![base.join("npm.cmd")]);
        assert!(which_candidates("", &dirs, &exts).is_empty());
    }
}
