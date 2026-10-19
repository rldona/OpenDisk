//! Full Disk Access detection (macOS).
//!
//! Without Full Disk Access, macOS hides Mail, Messages, Safari data, the
//! Trash and some app containers from OpenDisk, so scans under-count. There is
//! no API to ask for the permission; we probe a protected file and, if it is
//! denied, send the user to System Settings.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FdaState {
    Granted,
    Denied,
    /// None of the probe paths exist (or this is not macOS).
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct FdaStatus {
    pub state: FdaState,
    pub probed: Option<PathBuf>,
}

/// The System Settings pane where the user grants Full Disk Access.
pub const SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// Protected locations, most reliable first. The TCC database exists on every
/// Mac and can only be opened with Full Disk Access.
fn probes(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Mail"),
        home.join("Library/Safari"),
        home.join("Library/Messages"),
    ]
}

pub fn status(home: &Path) -> FdaStatus {
    if !cfg!(target_os = "macos") {
        return FdaStatus {
            state: FdaState::Unknown,
            probed: None,
        };
    }
    for p in probes(home) {
        let result = if p.is_dir() {
            std::fs::read_dir(&p).map(|_| ())
        } else {
            std::fs::File::open(&p).map(|_| ())
        };
        match result {
            Ok(()) => {
                return FdaStatus {
                    state: FdaState::Granted,
                    probed: Some(p),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                return FdaStatus {
                    state: FdaState::Denied,
                    probed: Some(p),
                }
            }
            Err(_) => continue, // missing: try the next probe
        }
    }
    FdaStatus {
        state: FdaState::Unknown,
        probed: None,
    }
}

/// Opens the Full Disk Access pane of System Settings.
pub fn open_settings() -> io::Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "only on macOS"));
    }
    let status = std::process::Command::new("/usr/bin/open").arg(SETTINGS_URL).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("could not open System Settings"))
    }
}
