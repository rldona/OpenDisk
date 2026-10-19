//! Local SQLite index of past scans, so a later version can answer "what grew
//! since the last scan". Only directory sizes down to a few levels are kept.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::scan::{DirNode, ScanResult};

/// How many levels below the scan root are stored.
pub const DEPTH: usize = 4;
/// Directories smaller than this are not stored.
pub const MIN_BYTES: u64 = 10_000_000;

pub struct Index {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct ScanSummary {
    pub id: i64,
    pub root: String,
    pub at: u64,
    pub total_bytes: u64,
}

impl Index {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS scans (
                 id INTEGER PRIMARY KEY, root TEXT NOT NULL, at INTEGER NOT NULL,
                 total_bytes INTEGER NOT NULL, files INTEGER NOT NULL, cancelled INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS dir_sizes (
                 scan_id INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
                 path TEXT NOT NULL, bytes INTEGER NOT NULL, files INTEGER NOT NULL,
                 PRIMARY KEY (scan_id, path));",
        )?;
        Ok(Self { conn })
    }

    pub fn record(&mut self, scan: &ScanResult) -> rusqlite::Result<i64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO scans (root, at, total_bytes, files, cancelled) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                scan.root.to_string_lossy(),
                crate::journal::now() as i64,
                scan.tree.size as i64,
                scan.tree.files as i64,
                scan.cancelled
            ],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut stmt = tx.prepare("INSERT INTO dir_sizes (scan_id, path, bytes, files) VALUES (?1, ?2, ?3, ?4)")?;
            let mut stack: Vec<(String, &DirNode, usize)> = vec![(String::new(), &scan.tree, 0)];
            while let Some((rel, node, depth)) = stack.pop() {
                stmt.execute(params![id, rel, node.size as i64, node.files as i64])?;
                if depth < DEPTH {
                    for c in node.children.iter().filter(|c| c.size >= MIN_BYTES) {
                        let child = if rel.is_empty() {
                            c.name.clone()
                        } else {
                            format!("{rel}/{}", c.name)
                        };
                        stack.push((child, c, depth + 1));
                    }
                }
            }
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn scans(&self) -> rusqlite::Result<Vec<ScanSummary>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, root, at, total_bytes FROM scans ORDER BY id DESC")?;
        let rows = stmt.query_map([], |r| {
            Ok(ScanSummary {
                id: r.get(0)?,
                root: r.get(1)?,
                at: r.get::<_, i64>(2)? as u64,
                total_bytes: r.get::<_, i64>(3)? as u64,
            })
        })?;
        rows.collect()
    }

    /// Size of `rel` (relative to the scan root) in a stored scan.
    pub fn dir_size(&self, scan_id: i64, rel: &str) -> rusqlite::Result<Option<u64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT bytes FROM dir_sizes WHERE scan_id = ?1 AND path = ?2")?;
        let mut rows = stmt.query(params![scan_id, rel])?;
        Ok(match rows.next()? {
            Some(r) => Some(r.get::<_, i64>(0)? as u64),
            None => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_scans() {
        let tmp = tempfile::tempdir().unwrap();
        let scan = ScanResult {
            root: "/x".into(),
            tree: DirNode {
                name: "/x".into(),
                size: 50_000_000,
                files: 3,
                children: vec![DirNode {
                    name: "big".into(),
                    size: 40_000_000,
                    files: 2,
                    ..Default::default()
                }],
                ..Default::default()
            },
            cancelled: false,
            stats: crate::scan::Progress::default().snapshot(),
            denied: vec![],
            duration_ms: 1,
        };
        let mut idx = Index::open(&tmp.path().join("index.sqlite")).unwrap();
        let id = idx.record(&scan).unwrap();
        assert_eq!(idx.scans().unwrap().len(), 1);
        assert_eq!(idx.dir_size(id, "big").unwrap(), Some(40_000_000));
        assert_eq!(idx.dir_size(id, "").unwrap(), Some(50_000_000));
    }
}
