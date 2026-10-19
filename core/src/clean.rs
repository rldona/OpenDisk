//! Plan → preview → execute → verify.
//!
//! 1. [`plan`] validates a selection against the rules and the denylist,
//!    records a fingerprint of each path and decides the action (Trash by
//!    default; direct delete only for regenerable caches when asked; an
//!    official command when a rule prefers one). Problems become `blockers`.
//! 2. The caller shows the plan (that is the preview).
//! 3. [`execute`] re-checks every item right before acting: still allowed,
//!    not a symlink, no symlinked ancestors, same inode and mtime as when
//!    planned, owning app not running. Then it acts (or only reports, in a
//!    dry run) and writes the journal.
//! 4. The report carries the real freed space from the `statfs` delta.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::journal::{self, Journal, JournalAction, JournalEntry};
use crate::paths;
use crate::rules::{CompiledRule, Rule, RuleSet, Safety};
use crate::size;
use crate::system::System;

/// What the user picked: a finding item (with its rule) or, with no rule, a
/// path picked by hand in the disk map (always moved to the Trash).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Selection {
    pub rule_id: Option<String>,
    pub path: PathBuf,
    /// Size shown to the user when they selected it.
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanOptions {
    /// Delete `regenerable` caches directly instead of moving them to the
    /// Trash (frees space immediately).
    pub delete_regenerable: bool,
    /// Allow rules marked `permanent` (emptying the Trash).
    pub allow_permanent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Trash,
    Delete,
    Command { argv: Vec<String>, description: String },
}

/// What a path looked like when planned. Any difference at execution time
/// means something else touched it, and the item is skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub dev: u64,
    pub ino: u64,
    pub is_dir: bool,
    pub mtime_ns: i128,
}

impl Fingerprint {
    pub fn of(md: &fs::Metadata) -> Self {
        let (dev, ino) = size::dev_ino(md);
        Self {
            dev,
            ino,
            is_dir: md.is_dir(),
            mtime_ns: mtime_ns(md),
        }
    }
}

#[cfg(unix)]
fn mtime_ns(md: &fs::Metadata) -> i128 {
    use std::os::unix::fs::MetadataExt;
    i128::from(md.mtime()) * 1_000_000_000 + i128::from(md.mtime_nsec())
}

#[cfg(not(unix))]
fn mtime_ns(md: &fs::Metadata) -> i128 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i128)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub rule_id: Option<String>,
    pub rule_name: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub safety: Safety,
    pub action: Action,
    pub fingerprint: Option<Fingerprint>,
    pub requires_closed: Vec<String>,
    pub requires_no_process: Vec<String>,
    /// Why this item will be skipped. Empty means it is ready.
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub home: PathBuf,
    pub created_at: u64,
    pub options: CleanOptions,
    pub items: Vec<PlanItem>,
    pub trash_bytes: u64,
    pub delete_bytes: u64,
    pub command_bytes: u64,
    pub blocked_bytes: u64,
}

fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{:x}-{:x}", nanos, std::process::id())
}

struct Running {
    bundles: HashSet<String>,
    processes: HashSet<String>,
}

impl Running {
    fn of(sys: &dyn System) -> Self {
        Self {
            bundles: sys.running_bundle_ids(),
            processes: sys.running_processes(),
        }
    }

    fn blockers(&self, bundles: &[String], processes: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for b in bundles {
            if self.bundles.contains(b) {
                out.push(format!("Quit the app {b} first"));
            }
        }
        for p in processes {
            // `ps -c` truncates names to 15/16 characters.
            let running = self.processes.contains(p)
                || (p.len() > 15
                    && self
                        .processes
                        .iter()
                        .any(|r| r.len() >= 15 && p.starts_with(r.as_str())));
            if running {
                out.push(format!("Stop the process `{p}` first"));
            }
        }
        out
    }
}

/// Checks that hold both when planning and right before acting.
fn static_checks(rule: Option<&Rule>, path: &Path, home: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(reason) = paths::deny_reason(path, home) {
        out.push(format!("Protected: {reason}"));
        return out;
    }
    if let Some(rule) = rule {
        if !CompiledRule::new(rule, home).matches(path) {
            out.push(format!("Not covered by rule {}", rule.id));
        }
    }
    if !paths::ancestors_are_real(path) {
        out.push("A parent folder is a symlink or missing".into());
    }
    out
}

/// Builds a plan for `selection`. Never modifies anything.
pub fn plan(selection: &[Selection], rules: &RuleSet, home: &Path, sys: &dyn System, opts: &CleanOptions) -> Plan {
    let running = Running::of(sys);
    let mut items = Vec::with_capacity(selection.len());
    let mut seen = HashSet::new();
    for sel in selection {
        if !seen.insert(sel.path.clone()) {
            continue;
        }
        let mut blockers = Vec::new();
        let rule = match &sel.rule_id {
            Some(id) => {
                let r = rules.get(id);
                if r.is_none() {
                    blockers.push(format!("Unknown rule {id}"));
                }
                r
            }
            None => None,
        };
        blockers.extend(static_checks(rule, &sel.path, home));
        let fingerprint = match fs::symlink_metadata(&sel.path) {
            Ok(md) if md.file_type().is_symlink() => {
                blockers.push("Is a symlink".into());
                None
            }
            Ok(md) => Some(Fingerprint::of(&md)),
            Err(e) => {
                blockers.push(format!("Cannot read: {e}"));
                None
            }
        };
        let (requires_closed, requires_no_process) = match rule {
            Some(r) => (
                CompiledRule::new(r, home).bundle_ids_for(&sel.path, home),
                r.requires_no_process.clone(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        blockers.extend(running.blockers(&requires_closed, &requires_no_process));

        let safety = rule.map_or(Safety::Personal, |r| r.safety);
        let action = match rule {
            Some(r)
                if r.prefer_command
                    .as_ref()
                    .is_some_and(|c| sys.command_exists(&c.argv[0])) =>
            {
                let c = r.prefer_command.clone().unwrap();
                Action::Command {
                    argv: c.argv,
                    description: c.description,
                }
            }
            Some(r) if r.permanent => {
                if !opts.allow_permanent {
                    blockers.push("Deletes permanently: confirm emptying the Trash to include it".into());
                }
                Action::Delete
            }
            Some(r) if r.safety == Safety::Regenerable && opts.delete_regenerable => Action::Delete,
            _ => Action::Trash,
        };
        items.push(PlanItem {
            rule_id: sel.rule_id.clone(),
            rule_name: rule.map_or_else(|| "Picked by you".to_string(), |r| r.name.clone()),
            path: sel.path.clone(),
            bytes: sel.bytes,
            safety,
            action,
            fingerprint,
            requires_closed,
            requires_no_process,
            blockers,
        });
    }
    let sum = |f: &dyn Fn(&PlanItem) -> bool| items.iter().filter(|i| f(i)).map(|i| i.bytes).sum();
    let ready = |i: &PlanItem| i.blockers.is_empty();
    Plan {
        id: new_id(),
        home: home.to_path_buf(),
        created_at: journal::now(),
        options: opts.clone(),
        trash_bytes: sum(&|i| ready(i) && i.action == Action::Trash),
        delete_bytes: sum(&|i| ready(i) && i.action == Action::Delete),
        command_bytes: sum(&|i| ready(i) && matches!(i.action, Action::Command { .. })),
        blocked_bytes: sum(&|i| !ready(i)),
        items,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Dry run: everything checked out and this would have happened.
    WouldClean,
    Trashed,
    Deleted,
    CommandRan,
    /// The rule's command already ran for another item of the same rule.
    CoveredByCommand,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemOutcome {
    pub path: PathBuf,
    pub rule_id: Option<String>,
    pub action: Action,
    pub bytes: u64,
    pub status: Status,
    pub message: Option<String>,
    pub trashed_to: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub run_id: String,
    pub plan_id: String,
    pub dry_run: bool,
    pub outcomes: Vec<ItemOutcome>,
    /// Available bytes before and after (`statfs`), and their difference:
    /// the space really freed. Can be lower than expected (APFS clones,
    /// hardlinks, local snapshots) or noisy (other apps writing meanwhile).
    pub available_before: u64,
    pub available_after: u64,
    pub freed_bytes: i64,
    /// Bytes moved to the Trash: freed only when the Trash is emptied.
    pub trashed_bytes: u64,
    pub deleted_bytes: u64,
    pub skipped: usize,
    pub failed: usize,
}

/// Re-checks an item right before acting on it.
fn recheck(item: &PlanItem, rules: &RuleSet, home: &Path, running: &Running) -> Result<fs::Metadata, String> {
    if !item.blockers.is_empty() {
        return Err(item.blockers.join("; "));
    }
    let rule = match &item.rule_id {
        Some(id) => Some(rules.get(id).ok_or_else(|| format!("Unknown rule {id}"))?),
        None => None,
    };
    let problems = static_checks(rule, &item.path, home);
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let md = fs::symlink_metadata(&item.path).map_err(|e| format!("Cannot read: {e}"))?;
    if md.file_type().is_symlink() {
        return Err("Became a symlink".into());
    }
    if item.fingerprint.as_ref() != Some(&Fingerprint::of(&md)) {
        return Err("Changed since the preview; preview again".into());
    }
    let busy = running.blockers(&item.requires_closed, &item.requires_no_process);
    if !busy.is_empty() {
        return Err(busy.join("; "));
    }
    Ok(md)
}

/// Executes `plan`. With `dry_run` it performs every check but changes nothing
/// and writes no journal entries.
pub fn execute(
    plan: &Plan,
    rules: &RuleSet,
    sys: &dyn System,
    journal: Option<&Journal>,
    dry_run: bool,
) -> io::Result<Report> {
    let home = &plan.home;
    let run_id = new_id();
    let running = Running::of(sys);
    let available_before = sys.available_bytes(home)?;
    let mut outcomes = Vec::with_capacity(plan.items.len());
    let mut commands_run: HashSet<Vec<String>> = HashSet::new();

    for item in &plan.items {
        let mut outcome = ItemOutcome {
            path: item.path.clone(),
            rule_id: item.rule_id.clone(),
            action: item.action.clone(),
            bytes: item.bytes,
            status: Status::Skipped,
            message: None,
            trashed_to: None,
        };
        let md = match recheck(item, rules, home, &running) {
            Ok(md) => md,
            Err(msg) => {
                outcome.message = Some(msg);
                outcomes.push(outcome);
                continue;
            }
        };
        if dry_run {
            outcome.status = Status::WouldClean;
            outcomes.push(outcome);
            continue;
        }
        let mut entry = JournalEntry {
            run_id: run_id.clone(),
            at: journal::now(),
            action: JournalAction::Deleted,
            rule_id: item.rule_id.clone(),
            path: item.path.clone(),
            trashed_to: None,
            bytes: item.bytes,
            detail: None,
        };
        let result = match &item.action {
            Action::Trash => sys.trash(&item.path).map(|to| {
                entry.action = JournalAction::Trashed;
                entry.trashed_to = Some(to.clone());
                outcome.trashed_to = Some(to);
                Status::Trashed
            }),
            Action::Delete => remove_tree(&item.path, size::dev_ino(&md).0).map(|()| Status::Deleted),
            Action::Command { argv, .. } => {
                if commands_run.insert(argv.clone()) {
                    entry.action = JournalAction::Command;
                    match sys.run(argv) {
                        Ok((true, out)) => {
                            entry.detail = Some(tail(&out));
                            Ok(Status::CommandRan)
                        }
                        Ok((false, out)) => {
                            Err(io::Error::other(format!("`{}` failed: {}", argv.join(" "), tail(&out))))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Ok(Status::CoveredByCommand)
                }
            }
        };
        match result {
            Ok(status) => {
                if status != Status::CoveredByCommand {
                    if let Some(j) = journal {
                        j.append(&entry)?;
                    }
                }
                outcome.status = status;
            }
            Err(e) => {
                outcome.status = Status::Failed;
                outcome.message = Some(e.to_string());
            }
        }
        outcomes.push(outcome);
    }

    if !dry_run {
        // SAFETY: sync(2) has no preconditions.
        #[cfg(unix)]
        unsafe {
            libc::sync()
        };
    }
    let available_after = sys.available_bytes(home)?;
    let total = |s: Status| outcomes.iter().filter(|o| o.status == s).map(|o| o.bytes).sum();
    Ok(Report {
        run_id,
        plan_id: plan.id.clone(),
        dry_run,
        trashed_bytes: total(Status::Trashed),
        deleted_bytes: total(Status::Deleted),
        skipped: outcomes.iter().filter(|o| o.status == Status::Skipped).count(),
        failed: outcomes.iter().filter(|o| o.status == Status::Failed).count(),
        available_before,
        available_after,
        freed_bytes: available_after as i64 - available_before as i64,
        outcomes,
    })
}

fn tail(s: &str) -> String {
    let s = s.trim();
    let start = s.len().saturating_sub(400);
    let start = (start..=s.len()).find(|&i| s.is_char_boundary(i)).unwrap_or(s.len());
    s[start..].to_string()
}

/// Deletes `path` recursively without following symlinks and without
/// crossing into another volume. Read-only directories (Go module cache) are
/// made writable first.
///
/// TODO(hardening): use openat/unlinkat relative to directory fds to close the
/// remaining check-then-act window.
pub fn remove_tree(path: &Path, dev: u64) -> io::Result<()> {
    let md = fs::symlink_metadata(path)?;
    if !md.is_dir() {
        return fs::remove_file(path); // unlinks symlinks themselves
    }
    if size::dev_ino(&md).0 != dev {
        return Err(io::Error::other(format!("{} is on another volume", path.display())));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = md.permissions().mode();
        if mode & 0o700 != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o700))?;
        }
    }
    for entry in fs::read_dir(path)? {
        remove_tree(&entry?.path(), dev)?;
    }
    fs::remove_dir(path)
}
