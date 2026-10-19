//! APFS local snapshots (read-only listing).
//!
//! Time Machine keeps hourly local snapshots for 24 hours, and macOS keeps one
//! around OS updates. Their blocks are not reported as used by any folder, so
//! "System Data" looks bigger than everything a scan finds. macOS deletes them
//! automatically when space runs low.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    TimeMachine,
    OsUpdate,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub name: String,
    pub kind: SnapshotKind,
    /// `YYYY-MM-DD-HHMMSS` for Time Machine snapshots.
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotReport {
    pub snapshots: Vec<Snapshot>,
    pub explanation: &'static str,
    /// Command that removes Time Machine local snapshots (run by the user).
    pub thin_command: &'static str,
    pub error: Option<String>,
}

pub const EXPLANATION: &str = "APFS local snapshots keep deleted and changed blocks so Time Machine \
and macOS updates can roll back. Their size is not shown by any folder (it counts as \"System Data\"), \
and space you free may not show up until the snapshot that still references it expires. Time Machine \
snapshots expire after 24 hours; macOS also removes them automatically when the disk is nearly full. \
OS-update snapshots disappear after the update finishes.";

pub fn parse(text: &str) -> Vec<Snapshot> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("com.apple."))
        .map(|name| {
            let (kind, date) = if let Some(rest) = name.strip_prefix("com.apple.TimeMachine.") {
                (
                    SnapshotKind::TimeMachine,
                    Some(rest.trim_end_matches(".local").to_string()),
                )
            } else if name.starts_with("com.apple.os.update") {
                (SnapshotKind::OsUpdate, None)
            } else {
                (SnapshotKind::Other, None)
            };
            Snapshot {
                name: name.to_string(),
                kind,
                date,
            }
        })
        .collect()
}

pub fn list() -> SnapshotReport {
    let mut report = SnapshotReport {
        snapshots: Vec::new(),
        explanation: EXPLANATION,
        thin_command: "tmutil thinlocalsnapshots / 999999999999 4",
        error: None,
    };
    if !cfg!(target_os = "macos") {
        report.error = Some("APFS snapshots are macOS-only".into());
        return report;
    }
    match std::process::Command::new("/usr/bin/tmutil")
        .args(["listlocalsnapshots", "/"])
        .output()
    {
        Ok(out) if out.status.success() => report.snapshots = parse(&String::from_utf8_lossy(&out.stdout)),
        Ok(out) => report.error = Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => report.error = Some(e.to_string()),
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tmutil() {
        let text =
            "Snapshots for disk /:\ncom.apple.TimeMachine.2026-10-19-101010.local\ncom.apple.os.update-ABCDEF123\n";
        let s = parse(text);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].kind, SnapshotKind::TimeMachine);
        assert_eq!(s[0].date.as_deref(), Some("2026-10-19-101010"));
        assert_eq!(s[1].kind, SnapshotKind::OsUpdate);
    }
}
