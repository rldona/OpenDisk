//! Expands the rules against a home folder and sizes what they match.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::paths;
use crate::rules::{expand, CompiledRule, MatchKind, PreferCommand, RuleSet, Safety};
use crate::scan::{self, ScanResult};
use crate::size;

/// One path a rule matched.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub path: PathBuf,
    pub bytes: u64,
    pub files: u64,
    pub is_dir: bool,
    /// Seconds since the Unix epoch.
    pub modified: Option<u64>,
    /// Bundle IDs that must be closed to clean this item.
    pub requires_closed: Vec<String>,
}

/// Everything one rule matched.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub rule_id: String,
    pub name: String,
    pub category: String,
    pub category_label: String,
    pub safety: Safety,
    pub explanation: String,
    pub prefer_command: Option<PreferCommand>,
    pub permanent: bool,
    pub requires_no_process: Vec<String>,
    pub total_bytes: u64,
    pub items: Vec<Item>,
}

/// Where sizes come from.
pub trait Sizer: Sync {
    /// `(allocated bytes, file count)` of a directory.
    fn dir_size(&self, path: &Path) -> (u64, u64);
}

/// Measures each directory with a fresh walk.
pub struct MeasureSizer;

impl Sizer for MeasureSizer {
    fn dir_size(&self, path: &Path) -> (u64, u64) {
        scan::measure(path).unwrap_or((0, 0))
    }
}

/// Reuses a finished scan when the path is inside it, measures otherwise.
pub struct ScanSizer<'a>(pub &'a ScanResult);

impl Sizer for ScanSizer<'_> {
    fn dir_size(&self, path: &Path) -> (u64, u64) {
        match self.0.find(path) {
            Some(node) if !self.0.cancelled => (node.size, node.files),
            _ => MeasureSizer.dir_size(path),
        }
    }
}

fn mtime_secs(md: &std::fs::Metadata) -> Option<u64> {
    md.modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Candidate paths of every rule, after priority resolution: a path claimed by
/// a higher-priority rule (or nested inside one, or containing one) is not
/// offered by a lower-priority rule. Sizes are not computed yet.
pub fn resolve(rules: &RuleSet, home: &Path) -> Vec<(usize, Vec<PathBuf>)> {
    let mut order: Vec<usize> = (0..rules.rules.len()).collect();
    order.sort_by_key(|&i| -rules.rules[i].priority); // stable: file order breaks ties
    let mut claimed: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    let now = SystemTime::now();
    for idx in order {
        let rule = &rules.rules[idx];
        let compiled = CompiledRule::new(rule, home);
        let mut mine = Vec::new();
        for pattern in &rule.paths {
            for path in expand(pattern, home) {
                if !compiled.matches(&path) || paths::deny_reason(&path, home).is_some() {
                    continue;
                }
                let Ok(md) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                let kind_ok = match rule.kind {
                    MatchKind::Any => true,
                    MatchKind::Dir => md.is_dir(),
                    MatchKind::File => md.is_file(),
                };
                if !kind_ok {
                    continue;
                }
                if let Some(days) = rule.min_age_days {
                    let cutoff = now - Duration::from_secs(u64::from(days) * 86_400);
                    match md.modified() {
                        Ok(m) if m <= cutoff => {}
                        _ => continue,
                    }
                }
                if claimed.iter().any(|c| c.starts_with(&path) || path.starts_with(c)) || mine.contains(&path) {
                    continue;
                }
                mine.push(path);
            }
        }
        claimed.extend(mine.iter().cloned());
        out.push((idx, mine));
    }
    out
}

/// Finds what every rule matches under `home`, sized with `sizer`. Findings
/// are sorted by recoverable size, items too. Empty matches are dropped.
pub fn find(rules: &RuleSet, home: &Path, sizer: &dyn Sizer) -> Vec<Finding> {
    use rayon::prelude::*;
    let resolved = resolve(rules, home);
    let mut findings: Vec<Finding> = resolved
        .into_par_iter()
        .filter_map(|(idx, paths)| {
            let rule = &rules.rules[idx];
            let compiled = CompiledRule::new(rule, home);
            let mut items: Vec<Item> = paths
                .into_par_iter()
                .filter_map(|path| {
                    let md = std::fs::symlink_metadata(&path).ok()?;
                    let (bytes, files) = if md.is_dir() {
                        sizer.dir_size(&path)
                    } else {
                        (size::info(&path, &md).allocated, 1)
                    };
                    (bytes > 0).then(|| Item {
                        requires_closed: compiled.bundle_ids_for(&path, home),
                        is_dir: md.is_dir(),
                        modified: mtime_secs(&md),
                        path,
                        bytes,
                        files,
                    })
                })
                .collect();
            if items.is_empty() {
                return None;
            }
            items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
            Some(Finding {
                rule_id: rule.id.clone(),
                name: rule.name.clone(),
                category: rule.category.clone(),
                category_label: crate::rules::category_label(&rule.category).to_string(),
                safety: rule.safety,
                explanation: rule.explanation.trim().to_string(),
                prefer_command: rule.prefer_command.clone(),
                permanent: rule.permanent,
                requires_no_process: rule.requires_no_process.clone(),
                total_bytes: items.iter().map(|i| i.bytes).sum(),
                items,
            })
        })
        .collect();
    findings.sort_by_key(|f| std::cmp::Reverse(f.total_bytes));
    findings
}
