//! Append-only journal of every cleaning action, used for undo and audits.
//!
//! One JSON object per line in `<data dir>/journal.jsonl`.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalAction {
    Trashed,
    Deleted,
    Command,
    Restored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Groups the entries of one execution.
    pub run_id: String,
    /// Seconds since the Unix epoch.
    pub at: u64,
    pub action: JournalAction,
    pub rule_id: Option<String>,
    pub path: PathBuf,
    /// Where a trashed item went; needed to put it back.
    pub trashed_to: Option<PathBuf>,
    pub bytes: u64,
    pub detail: Option<String>,
}

/// Restored paths, and paths that could not be restored with the reason.
pub type UndoResult = (Vec<PathBuf>, Vec<(PathBuf, String)>);

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn open(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        Ok(Self {
            path: dir.join("journal.jsonl"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, entry: &JournalEntry) -> io::Result<()> {
        let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        f.write_all(line.as_bytes())?;
        f.sync_data()
    }

    pub fn entries(&self) -> io::Result<Vec<JournalEntry>> {
        let f = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        Ok(io::BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| serde_json::from_str(&l).ok())
            .collect())
    }

    /// Puts back everything a run moved to the Trash, if the original location
    /// is still free. Returns the restored paths; items that could not be
    /// restored are reported as errors in the second list.
    pub fn undo(&self, run_id: &str) -> io::Result<UndoResult> {
        let entries = self.entries()?;
        let already: Vec<&PathBuf> = entries
            .iter()
            .filter(|e| e.action == JournalAction::Restored && e.run_id == run_id)
            .map(|e| &e.path)
            .collect();
        let mut restored = Vec::new();
        let mut failed = Vec::new();
        for e in entries
            .iter()
            .filter(|e| e.run_id == run_id && e.action == JournalAction::Trashed)
        {
            if already.contains(&&e.path) {
                continue;
            }
            let Some(from) = &e.trashed_to else { continue };
            let result = if fs::symlink_metadata(&e.path).is_ok() {
                Err("something already exists at the original location".to_string())
            } else if fs::symlink_metadata(from).is_err() {
                Err("no longer in the Trash".to_string())
            } else {
                fs::rename(from, &e.path).map_err(|err| err.to_string())
            };
            if result.is_ok() {
                crate::system::forget_trash_info(from);
            }
            match result {
                Ok(()) => {
                    self.append(&JournalEntry {
                        action: JournalAction::Restored,
                        at: now(),
                        trashed_to: None,
                        detail: None,
                        ..e.clone()
                    })?;
                    restored.push(e.path.clone());
                }
                Err(msg) => failed.push((e.path.clone(), msg)),
            }
        }
        Ok((restored, failed))
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
