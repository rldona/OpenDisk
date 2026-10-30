//! End-to-end CLI tests against a fixture home. `HOME` and `USERPROFILE` are
//! also pointed at the fixture so nothing (journal, index) is written to the
//! real home folder.

use std::fs;
use std::path::Path;
use std::process::Command;

fn opendisk(home: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_opendisk"))
        .env("HOME", home)
        .env("USERPROFILE", home)
        .arg("--home")
        .arg(home)
        .args(args)
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = opendisk_core::paths::canonicalize(tmp.path()).unwrap();
    let cache = if cfg!(windows) {
        home.join("AppData/Local/npm-cache/_cacache")
    } else {
        home.join(".npm/_cacache")
    };
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("blob"), vec![1u8; 2_000_000]).unwrap();
    (tmp, home, cache)
}

#[test]
fn scan_findings_and_dry_run_clean() {
    let (_tmp, home, cache) = fixture();
    let (ok, out) = opendisk(&home, &["scan", "--no-index"]);
    assert!(ok, "{out}");
    assert!(out.contains(if cfg!(windows) { "AppData" } else { ".npm" }), "{out}");

    let (ok, out) = opendisk(&home, &["findings"]);
    assert!(ok, "{out}");
    assert!(out.contains("npm cache"), "{out}");

    let (ok, out) = opendisk(&home, &["clean"]);
    assert!(ok, "{out}");
    assert!(out.contains("Dry run"), "{out}");
    assert!(cache.join("blob").exists(), "dry run is the default");
}

#[test]
fn execute_requires_confirmation_without_tty() {
    let (_tmp, home, cache) = fixture();
    let (ok, out) = opendisk(&home, &["clean", "--execute", "--delete-regenerable"]);
    assert!(!ok, "{out}");
    assert!(cache.join("blob").exists());

    let (ok, out) = opendisk(&home, &["clean", "--execute", "--delete-regenerable", "--yes"]);
    assert!(ok, "{out}");
    assert!(!cache.exists(), "{out}");
    assert!(out.contains("Space really freed"), "{out}");
}

#[test]
fn unknown_rule_is_an_error() {
    let (_tmp, home, _cache) = fixture();
    let (ok, _) = opendisk(&home, &["clean", "--rule", "nope"]);
    assert!(!ok);
}

#[test]
fn lists_large_files_and_dry_runs_their_clean() {
    let (_tmp, home, _cache) = fixture();
    let big = home.join("Downloads/big-video.mkv");
    fs::create_dir_all(big.parent().unwrap()).unwrap();
    fs::write(&big, vec![7u8; 3_000_000]).unwrap();

    let (ok, out) = opendisk(&home, &["large", "--min", "2MB"]);
    assert!(ok, "{out}");
    assert!(out.contains("big-video.mkv"), "{out}");
    assert!(out.contains("cleanable"), "{out}");

    let (ok, out) = opendisk(&home, &["large", "--min", "2MB", "--clean"]);
    assert!(ok, "{out}");
    assert!(out.contains("Dry run"), "{out}");
    assert!(big.exists(), "the dry run must not remove anything");
}
