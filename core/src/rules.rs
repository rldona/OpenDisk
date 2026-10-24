//! Declarative cleaning rules.
//!
//! Rules live in YAML files embedded in the binary (`core/rules/*.yaml`). A rule
//! says *where* (path globs relative to `~`), *how safe* it is, *why* it is
//! safe and *what must be closed* first. Only paths a rule covers can be
//! cleaned (plus paths the user explicitly picks, which always go to Trash).

use std::path::Path;
use std::sync::OnceLock;

use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

/// How safe it is to remove what a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Safety {
    /// A cache the owning tool rebuilds on demand. May be deleted directly.
    Regenerable,
    /// Rebuildable but costly (big downloads, archives) or worth a look.
    Review,
    /// The user's own data. Only ever moved to the Trash.
    Personal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    #[default]
    Any,
    Dir,
    File,
}

/// An official tool that cleans better than deleting files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferCommand {
    /// Program and arguments, run without a shell.
    pub argv: Vec<String>,
    /// What the command does, shown in the preview.
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub category: String,
    pub safety: Safety,
    pub explanation: String,
    /// Globs relative to `~` (`*`, `?`, `[..]`, `{a,b}` within one component).
    pub paths: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub kind: MatchKind,
    /// Bundle IDs of apps that must not be running.
    #[serde(default)]
    pub requires_closed: Vec<String>,
    /// Process names (as in `ps -c`) that must not be running.
    #[serde(default)]
    pub requires_no_process: Vec<String>,
    /// Index of the home-relative path component that is a bundle ID which
    /// must not be running (e.g. 2 for `~/Library/Caches/<bundle id>`).
    #[serde(default)]
    pub bundle_id_component: Option<usize>,
    #[serde(default)]
    pub prefer_command: Option<PreferCommand>,
    /// Only match items whose modification time is older than this.
    #[serde(default)]
    pub min_age_days: Option<u32>,
    /// Higher wins when two rules match the same or nested paths.
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Cleaning removes data for good (e.g. emptying the Trash).
    #[serde(default)]
    pub permanent: bool,
    /// Paths (with `~`) this rule must match; used by the test suite.
    #[serde(default)]
    pub examples: Vec<String>,
    /// Paths this rule must not match; used by the test suite.
    #[serde(default)]
    pub non_examples: Vec<String>,
}

fn default_priority() -> i32 {
    50
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    version: u32,
    rules: Vec<Rule>,
}

/// Categories the UI knows how to label, in display order.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("ai-models", "AI models"),
    ("xcode", "Xcode & simulators"),
    ("javascript", "JavaScript"),
    ("jvm", "Java & Android"),
    ("python", "Python"),
    ("rust-go", "Rust & Go"),
    ("containers", "Containers"),
    ("homebrew", "Homebrew"),
    ("app-caches", "App caches"),
    ("browsers", "Browsers"),
    ("logs", "Logs"),
    ("downloads", "Downloads"),
    ("backups", "Device backups"),
    ("trash", "Trash"),
];

pub fn category_label(id: &str) -> &str {
    CATEGORIES.iter().find(|(c, _)| *c == id).map_or(id, |(_, label)| label)
}

#[derive(Debug)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
}

#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("invalid rules YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("rule {0}: {1}")]
    Invalid(String, String),
}

impl RuleSet {
    pub fn from_yaml(yaml: &str) -> Result<Self, RuleError> {
        let file: RuleFile = serde_yaml::from_str(yaml)?;
        if file.version != 1 {
            return Err(RuleError::Invalid(
                "<file>".into(),
                format!("unsupported version {}", file.version),
            ));
        }
        let set = RuleSet { rules: file.rules };
        set.validate()?;
        Ok(set)
    }

    fn validate(&self) -> Result<(), RuleError> {
        let mut ids = std::collections::HashSet::new();
        for r in &self.rules {
            let bad = |msg: &str| Err(RuleError::Invalid(r.id.clone(), msg.into()));
            if !ids.insert(&r.id) {
                return bad("duplicate id");
            }
            if r.paths.is_empty() {
                return bad("no paths");
            }
            for p in r.paths.iter().chain(&r.exclude) {
                if !p.starts_with("~/") {
                    return bad("paths must start with ~/");
                }
                if p.split('/').any(|c| c == ".." || c == "." || c.contains("**")) {
                    return bad("paths may not contain '.', '..' or '**'");
                }
                if let Err(e) = Glob::new(p) {
                    return bad(&format!("bad glob {p}: {e}"));
                }
            }
            if !CATEGORIES.iter().any(|(c, _)| *c == r.category) {
                return bad("unknown category");
            }
            if r.explanation.trim().len() < 20 {
                return bad("explanation too short");
            }
            if let Some(cmd) = &r.prefer_command {
                if cmd.argv.is_empty() {
                    return bad("prefer_command.argv is empty");
                }
            }
        }
        Ok(())
    }

    /// The rules shipped with OpenDisk for the current platform.
    pub fn builtin() -> &'static RuleSet {
        if cfg!(target_os = "macos") {
            Self::macos()
        } else {
            Self::linux()
        }
    }

    /// The macOS rules (`rules/macos.yaml`), available on every platform for tests.
    pub fn macos() -> &'static RuleSet {
        static SET: OnceLock<RuleSet> = OnceLock::new();
        SET.get_or_init(|| RuleSet::from_yaml(include_str!("../rules/macos.yaml")).expect("built-in rules are valid"))
    }

    /// The Linux rules (`rules/linux.yaml`), available on every platform for tests.
    pub fn linux() -> &'static RuleSet {
        static SET: OnceLock<RuleSet> = OnceLock::new();
        SET.get_or_init(|| RuleSet::from_yaml(include_str!("../rules/linux.yaml")).expect("built-in rules are valid"))
    }

    pub fn get(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id == id)
    }
}

/// A rule's globs compiled against a concrete home folder.
pub struct CompiledRule<'a> {
    pub rule: &'a Rule,
    include: Vec<GlobMatcher>,
    exclude: GlobSet,
}

fn glob(pattern: &str) -> Glob {
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
        .expect("validated glob")
}

impl<'a> CompiledRule<'a> {
    pub fn new(rule: &'a Rule, home: &Path) -> Self {
        let include = rule
            .paths
            .iter()
            .map(|p| glob(&escape_home(home, p)).compile_matcher())
            .collect();
        let mut ex = GlobSetBuilder::new();
        for p in &rule.exclude {
            ex.add(glob(&escape_home(home, p)));
        }
        Self {
            rule,
            include,
            exclude: ex.build().expect("validated globs"),
        }
    }

    /// Whether `path` is one of this rule's targets (matches a pattern and no
    /// exclusion). Does not touch the filesystem.
    pub fn matches(&self, path: &Path) -> bool {
        self.include.iter().any(|g| g.is_match(path)) && !self.exclude.is_match(path)
    }

    /// Bundle IDs that must not be running to clean `path`.
    pub fn bundle_ids_for(&self, path: &Path, home: &Path) -> Vec<String> {
        let mut ids = self.rule.requires_closed.clone();
        if let (Some(idx), Ok(rel)) = (self.rule.bundle_id_component, path.strip_prefix(home)) {
            if let Some(comp) = rel.components().nth(idx) {
                let s = comp.as_os_str().to_string_lossy();
                if looks_like_bundle_id(&s) && !ids.iter().any(|i| *i == s) {
                    ids.push(s.into_owned());
                }
            }
        }
        ids
    }
}

/// Glob-escapes the home prefix (it may contain `[`, `{`...) and expands `~`.
fn escape_home(home: &Path, pattern: &str) -> String {
    let rest = pattern.strip_prefix("~/").unwrap_or(pattern);
    let home = home.to_string_lossy();
    let mut escaped = String::with_capacity(home.len() + rest.len() + 1);
    for c in home.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped.push('/');
    escaped.push_str(rest);
    escaped
}

fn looks_like_bundle_id(s: &str) -> bool {
    s.split('.').count() >= 3
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// Expands `~/`-relative glob patterns against `home` without following
/// symlinks anywhere along the way. Returns existing, non-symlink paths.
pub fn expand(pattern: &str, home: &Path) -> Vec<std::path::PathBuf> {
    let rel = pattern.strip_prefix("~/").unwrap_or(pattern);
    let mut current = vec![home.to_path_buf()];
    let comps: Vec<&str> = rel.split('/').filter(|c| !c.is_empty()).collect();
    for (i, comp) in comps.iter().enumerate() {
        let last = i + 1 == comps.len();
        let wildcard = comp.contains(['*', '?', '[', '{']);
        let matcher = wildcard.then(|| glob(comp).compile_matcher());
        let mut next = Vec::new();
        for dir in &current {
            if let Some(m) = &matcher {
                let Ok(entries) = std::fs::read_dir(dir) else { continue };
                for e in entries.flatten() {
                    let name = e.file_name();
                    let name_s = name.to_string_lossy();
                    if matches!(name_s.as_ref(), ".DS_Store" | ".localized") {
                        continue;
                    }
                    if m.is_match(name_s.as_ref()) {
                        next.push(e.path());
                    }
                }
            } else {
                next.push(dir.join(comp));
            }
        }
        // Keep only real entries; intermediate ones must be real directories.
        next.retain(|p| match std::fs::symlink_metadata(p) {
            Ok(md) if md.file_type().is_symlink() => false,
            Ok(md) => last || md.is_dir(),
            Err(_) => false,
        });
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current.sort();
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_rules_load_and_validate() {
        let set = RuleSet::builtin();
        assert!(
            set.rules.len() >= 40,
            "ships at least 40 rules, has {}",
            set.rules.len()
        );
        for r in &set.rules {
            assert!(!r.examples.is_empty(), "rule {} needs at least one example", r.id);
        }
    }

    #[test]
    fn rejects_bad_rules() {
        let base = |paths: &str| {
            format!(
                "version: 1\nrules:\n- id: x\n  name: X\n  category: logs\n  safety: review\n  explanation: a long enough explanation\n  paths: [{paths}]\n"
            )
        };
        assert!(RuleSet::from_yaml(&base("\"~/Library/Logs/*\"")).is_ok());
        assert!(RuleSet::from_yaml(&base("\"/etc/*\"")).is_err());
        assert!(RuleSet::from_yaml(&base("\"~/a/../b\"")).is_err());
        assert!(RuleSet::from_yaml(&base("\"~/**/x\"")).is_err());
        let unknown_field = base("\"~/x\"") + "  delete_everything: true\n";
        assert!(RuleSet::from_yaml(&unknown_field).is_err());
    }

    #[test]
    fn matching_escapes_home() {
        let rule = RuleSet::macos().get("user-caches").unwrap();
        let home = Path::new("/Users/we[ir]d");
        let c = CompiledRule::new(rule, home);
        assert!(c.matches(Path::new("/Users/we[ir]d/Library/Caches/com.foo.bar")));
        assert!(!c.matches(Path::new("/Users/wid/Library/Caches/com.foo.bar")));
        assert!(!c.matches(Path::new("/Users/we[ir]d/Library/Caches/com.foo.bar/sub")));
        assert_eq!(
            c.bundle_ids_for(Path::new("/Users/we[ir]d/Library/Caches/com.foo.bar"), home),
            vec!["com.foo.bar".to_string()]
        );
    }
}
