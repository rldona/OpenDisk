//! End-to-end CLI tests against a fixture home. `HOME` is also pointed at the
//! fixture so nothing (journal, index) is written to the real home folder.

use std::fs;
use std::path::Path;
use std::process::Command;

fn opendisk(home: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_opendisk"))
        .env("HOME", home)
        .arg("--home")
        .arg(home)
        .args(args)
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(tmp.path()).unwrap();
    let cache = home.join(".npm/_cacache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("blob"), vec![1u8; 2_000_000]).unwrap();
    (tmp, home)
}

#[test]
fn scan_findings_and_dry_run_clean() {
    let (_tmp, home) = fixture();
    let (ok, out) = opendisk(&home, &["scan", "--no-index"]);
    assert!(ok, "{out}");
    assert!(out.contains(".npm"), "{out}");

    let (ok, out) = opendisk(&home, &["findings"]);
    assert!(ok, "{out}");
    assert!(out.contains("npm cache"), "{out}");

    let (ok, out) = opendisk(&home, &["clean"]);
    assert!(ok, "{out}");
    assert!(out.contains("Dry run"), "{out}");
    assert!(home.join(".npm/_cacache/blob").exists(), "dry run is the default");
}

#[test]
fn execute_requires_confirmation_without_tty() {
    let (_tmp, home) = fixture();
    let (ok, out) = opendisk(&home, &["clean", "--execute", "--delete-regenerable"]);
    assert!(!ok, "{out}");
    assert!(home.join(".npm/_cacache/blob").exists());

    let (ok, out) = opendisk(&home, &["clean", "--execute", "--delete-regenerable", "--yes"]);
    assert!(ok, "{out}");
    assert!(!home.join(".npm/_cacache").exists(), "{out}");
    assert!(out.contains("Space really freed"), "{out}");
}

#[test]
fn unknown_rule_is_an_error() {
    let (_tmp, home) = fixture();
    let (ok, _) = opendisk(&home, &["clean", "--rule", "nope"]);
    assert!(!ok);
}
