//! Every built-in rule is tested against its own `examples` and
//! `non_examples`, all placed together in one fake home folder so priority
//! resolution between rules is tested too. Nothing here touches the real home.

use std::fs;
use std::path::{Path, PathBuf};

use opendisk_core::findings::{self, MeasureSizer};
use opendisk_core::paths::expand_tilde;
use opendisk_core::rules::{MatchKind, RuleSet};

fn fake_home() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(tmp.path()).unwrap().join("home");
    fs::create_dir_all(&home).unwrap();
    (tmp, home)
}

/// Creates `path` as a file or as a directory with a payload, and makes it old
/// enough for `min_age_days` rules.
fn materialize(path: &Path, as_file: bool) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = if as_file {
        path.to_path_buf()
    } else {
        fs::create_dir_all(path).unwrap();
        path.join("payload.bin")
    };
    if !file.exists() {
        fs::write(&file, vec![1u8; 8192]).unwrap();
    }
    let old = filetime::FileTime::from_unix_time(1_000_000_000, 0);
    filetime::set_file_mtime(&file, old).unwrap();
    filetime::set_file_mtime(path, old).unwrap();
}

fn is_file_example(kind: MatchKind, example: &str) -> bool {
    kind == MatchKind::File || example.ends_with("Docker.raw")
}

#[test]
fn every_macos_rule_claims_its_examples() {
    check_examples(RuleSet::macos());
}

#[test]
fn every_linux_rule_claims_its_examples() {
    check_examples(RuleSet::linux());
}

fn check_examples(rules: &RuleSet) {
    let (_tmp, home) = fake_home();
    for rule in &rules.rules {
        for ex in rule.examples.iter().chain(&rule.non_examples) {
            materialize(&expand_tilde(ex, &home), is_file_example(rule.kind, ex));
        }
    }
    let found = findings::find(rules, &home, &MeasureSizer);
    let owner = |path: &Path| {
        found
            .iter()
            .find(|f| f.items.iter().any(|i| i.path == path))
            .map(|f| f.rule_id.clone())
    };
    let mut failures = Vec::new();
    for rule in &rules.rules {
        for ex in &rule.examples {
            let path = expand_tilde(ex, &home);
            match owner(&path) {
                Some(id) if id == rule.id => {}
                other => failures.push(format!("{}: example {ex} claimed by {other:?}", rule.id)),
            }
        }
        for ex in &rule.non_examples {
            let path = expand_tilde(ex, &home);
            if owner(&path).as_deref() == Some(rule.id.as_str()) {
                failures.push(format!("{}: non-example {ex} was matched", rule.id));
            }
        }
    }
    assert!(failures.is_empty(), "rule failures:\n{}", failures.join("\n"));
}

#[test]
fn symlinked_folders_are_never_matched_or_followed() {
    let rules = RuleSet::macos();
    let (tmp, home) = fake_home();
    // A real folder outside home that a symlink in home points to.
    let outside = tmp.path().join("outside-data");
    materialize(&outside.join("precious"), false);
    fs::create_dir_all(home.join("Library/Caches")).unwrap();
    std::os::unix::fs::symlink(&outside, home.join("Library/Caches/com.example.link")).unwrap();
    // ~/.npm itself a symlink: nothing below it may match.
    std::os::unix::fs::symlink(&outside, home.join(".npm")).unwrap();
    fs::create_dir_all(outside.join("_cacache")).unwrap();

    let found = findings::find(rules, &home, &MeasureSizer);
    for f in &found {
        for i in &f.items {
            assert!(
                !i.path.starts_with(home.join("Library/Caches/com.example.link")),
                "{:?}",
                i.path
            );
            assert!(!i.path.starts_with(home.join(".npm")), "{:?}", i.path);
        }
    }
}

#[test]
fn young_installers_are_left_alone() {
    let rules = RuleSet::macos();
    let (_tmp, home) = fake_home();
    let dmg = home.join("Downloads/New.dmg");
    fs::create_dir_all(dmg.parent().unwrap()).unwrap();
    fs::write(&dmg, vec![0u8; 8192]).unwrap();
    let found = findings::find(rules, &home, &MeasureSizer);
    assert!(found.iter().all(|f| f.rule_id != "old-installers"));
}

#[test]
fn findings_sorted_and_sized() {
    let rules = RuleSet::macos();
    let (_tmp, home) = fake_home();
    materialize(&home.join(".npm/_cacache"), false);
    fs::write(home.join(".npm/_cacache/more.bin"), vec![0u8; 1 << 20]).unwrap();
    materialize(&home.join("Library/Caches/pip"), false);
    let found = findings::find(rules, &home, &MeasureSizer);
    assert_eq!(found[0].rule_id, "npm-cache");
    assert!(found[0].total_bytes >= 1 << 20);
    assert!(found.windows(2).all(|w| w[0].total_bytes >= w[1].total_bytes));
}
