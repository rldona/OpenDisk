//! Cleaner tests against fixture trees in temp dirs, with a fake system so the
//! real Trash, real apps and real commands are never touched.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use opendisk_core::clean::{self, Action, CleanOptions, Selection, Status};
use opendisk_core::journal::Journal;
use opendisk_core::rules::RuleSet;
use opendisk_core::system::System;

struct FakeSystem {
    trash_dir: PathBuf,
    bundles: HashSet<String>,
    processes: HashSet<String>,
    commands: HashSet<String>,
    ran: Mutex<Vec<Vec<String>>>,
}

impl FakeSystem {
    fn new(trash_dir: PathBuf) -> Self {
        fs::create_dir_all(&trash_dir).unwrap();
        Self {
            trash_dir,
            bundles: HashSet::new(),
            processes: HashSet::new(),
            commands: HashSet::new(),
            ran: Mutex::default(),
        }
    }
}

impl System for FakeSystem {
    fn trash(&self, path: &Path) -> io::Result<PathBuf> {
        let to = self.trash_dir.join(path.file_name().unwrap());
        fs::rename(path, &to)?;
        Ok(to)
    }
    fn running_bundle_ids(&self) -> HashSet<String> {
        self.bundles.clone()
    }
    fn running_processes(&self) -> HashSet<String> {
        self.processes.clone()
    }
    fn available_bytes(&self, path: &Path) -> io::Result<u64> {
        opendisk_core::system::available_bytes(path)
    }
    fn command_exists(&self, program: &str) -> bool {
        self.commands.contains(program)
    }
    fn run(&self, argv: &[String]) -> io::Result<(bool, String)> {
        self.ran.lock().unwrap().push(argv.to_vec());
        Ok((true, "ok".into()))
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    sys: FakeSystem,
    journal: Journal,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let sys = FakeSystem::new(root.join("fake-trash"));
    let journal = Journal::open(&root.join("data")).unwrap();
    Fixture {
        _tmp: tmp,
        home,
        sys,
        journal,
    }
}

fn make_dir(path: &Path, bytes: usize) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("blob.bin"), vec![3u8; bytes]).unwrap();
}

fn sel(rule: &str, path: &Path) -> Selection {
    Selection {
        rule_id: Some(rule.into()),
        path: path.to_path_buf(),
        bytes: 1,
    }
}

fn rules() -> &'static RuleSet {
    RuleSet::builtin()
}

#[test]
fn dry_run_changes_nothing() {
    let f = fixture();
    let target = f.home.join("Library/Caches/com.example.app");
    make_dir(&target, 4096);
    let plan = clean::plan(
        &[sel("user-caches", &target)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(plan.items[0].blockers.is_empty(), "{:?}", plan.items[0].blockers);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), true).unwrap();
    assert!(report.dry_run);
    assert_eq!(report.outcomes[0].status, Status::WouldClean);
    assert!(target.join("blob.bin").exists());
    assert!(f.journal.entries().unwrap().is_empty(), "dry run writes no journal");
}

#[test]
fn trash_by_default_and_undo() {
    let f = fixture();
    let target = f.home.join("Library/Caches/com.example.app");
    make_dir(&target, 4096);
    let plan = clean::plan(
        &[sel("user-caches", &target)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert_eq!(
        plan.items[0].action,
        Action::Trash,
        "regenerable still goes to Trash by default"
    );
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Trashed);
    assert!(!target.exists());
    assert!(report.trashed_bytes > 0);

    let (restored, failed) = f.journal.undo(&report.run_id).unwrap();
    assert_eq!(restored, vec![target.clone()]);
    assert!(failed.is_empty());
    assert!(target.join("blob.bin").exists());
    // Undo is idempotent.
    let (again, _) = f.journal.undo(&report.run_id).unwrap();
    assert!(again.is_empty());
}

#[test]
fn delete_regenerable_frees_space_now() {
    let f = fixture();
    let target = f.home.join(".npm/_cacache");
    make_dir(&target, 1 << 20);
    // A read-only nested dir (like Go's module cache) must still be removable.
    make_dir(&target.join("ro"), 10);
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(target.join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
    let opts = CleanOptions {
        delete_regenerable: true,
        ..Default::default()
    };
    let plan = clean::plan(&[sel("npm-cache", &target)], rules(), &f.home, &f.sys, &opts);
    assert_eq!(plan.items[0].action, Action::Delete);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(
        report.outcomes[0].status,
        Status::Deleted,
        "{:?}",
        report.outcomes[0].message
    );
    assert!(!target.exists());
}

#[test]
fn review_items_are_never_deleted_directly() {
    let f = fixture();
    let target = f.home.join(".cache/huggingface/hub/models--x--y");
    make_dir(&target, 4096);
    let opts = CleanOptions {
        delete_regenerable: true,
        allow_permanent: true,
    };
    let plan = clean::plan(&[sel("huggingface-hub", &target)], rules(), &f.home, &f.sys, &opts);
    assert_eq!(plan.items[0].action, Action::Trash);
}

#[test]
fn refuses_symlink_target() {
    let f = fixture();
    let outside = f.home.parent().unwrap().join("outside");
    make_dir(&outside, 4096);
    fs::create_dir_all(f.home.join("Library/Caches")).unwrap();
    let link = f.home.join("Library/Caches/com.example.link");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let plan = clean::plan(
        &[sel("user-caches", &link)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(!plan.items[0].blockers.is_empty());
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Skipped);
    assert!(outside.join("blob.bin").exists());
    assert!(fs::symlink_metadata(&link).is_ok());
}

#[test]
fn refuses_symlinked_ancestor() {
    let f = fixture();
    // ~/.npm is a symlink to a folder outside home that has a _cacache.
    let outside = f.home.parent().unwrap().join("outside-npm");
    make_dir(&outside.join("_cacache"), 4096);
    std::os::unix::fs::symlink(&outside, f.home.join(".npm")).unwrap();
    let target = f.home.join(".npm/_cacache");
    let opts = CleanOptions {
        delete_regenerable: true,
        ..Default::default()
    };
    let plan = clean::plan(&[sel("npm-cache", &target)], rules(), &f.home, &f.sys, &opts);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Skipped);
    assert!(outside.join("_cacache/blob.bin").exists());
}

#[test]
fn symlinks_inside_a_deleted_tree_are_not_followed() {
    let f = fixture();
    let outside = f.home.parent().unwrap().join("precious");
    make_dir(&outside, 4096);
    let target = f.home.join(".npm/_cacache");
    make_dir(&target, 4096);
    std::os::unix::fs::symlink(&outside, target.join("evil-link")).unwrap();
    let opts = CleanOptions {
        delete_regenerable: true,
        ..Default::default()
    };
    let plan = clean::plan(&[sel("npm-cache", &target)], rules(), &f.home, &f.sys, &opts);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Deleted);
    assert!(outside.join("blob.bin").exists(), "symlink target survives");
}

#[test]
fn hardlinked_files_survive_through_other_links() {
    let f = fixture();
    let target = f.home.join(".npm/_cacache");
    make_dir(&target, 4096);
    let keep = f.home.join("project-file.bin");
    fs::hard_link(target.join("blob.bin"), &keep).unwrap();
    let opts = CleanOptions {
        delete_regenerable: true,
        ..Default::default()
    };
    let plan = clean::plan(&[sel("npm-cache", &target)], rules(), &f.home, &f.sys, &opts);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Deleted);
    assert_eq!(fs::read(&keep).unwrap().len(), 4096, "other hardlink keeps the data");
}

#[test]
fn refuses_when_changed_after_preview() {
    let f = fixture();
    let target = f.home.join("Library/Caches/com.example.app");
    make_dir(&target, 4096);
    let plan = clean::plan(
        &[sel("user-caches", &target)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    // Something replaces the folder between preview and execution.
    fs::remove_dir_all(&target).unwrap();
    make_dir(&target, 10);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Skipped);
    assert!(report.outcomes[0].message.as_deref().unwrap().contains("Changed"));
    assert!(target.exists());
}

#[test]
fn refuses_when_swapped_for_symlink_after_preview() {
    let f = fixture();
    let target = f.home.join("Library/Caches/com.example.app");
    make_dir(&target, 4096);
    let plan = clean::plan(
        &[sel("user-caches", &target)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    let outside = f.home.parent().unwrap().join("outside");
    make_dir(&outside, 4096);
    fs::remove_dir_all(&target).unwrap();
    std::os::unix::fs::symlink(&outside, &target).unwrap();
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Skipped);
    assert!(outside.join("blob.bin").exists());
}

#[test]
fn refuses_paths_outside_the_rule_and_denylisted_paths() {
    let f = fixture();
    let docs = f.home.join("Documents/thesis");
    make_dir(&docs, 4096);
    let mail = f.home.join("Library/Mail/V10");
    make_dir(&mail, 4096);
    let caches_root = f.home.join("Library/Caches");
    let plan = clean::plan(
        &[
            sel("user-caches", &docs),
            sel("user-caches", &mail),
            sel("user-caches", &caches_root),
            sel("no-such-rule", &docs),
        ],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(plan.items.iter().all(|i| !i.blockers.is_empty()), "{:#?}", plan.items);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert!(report.outcomes.iter().all(|o| o.status == Status::Skipped));
    assert!(docs.exists() && mail.exists());
}

#[test]
fn blocks_while_owning_app_runs() {
    let mut f = fixture();
    let target = f.home.join("Library/Application Support/Slack/Cache");
    make_dir(&target, 4096);
    f.sys.bundles.insert("com.tinyspeck.slackmacgap".into());
    let plan = clean::plan(
        &[sel("slack-caches", &target)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(plan.items[0]
        .blockers
        .iter()
        .any(|b| b.contains("com.tinyspeck.slackmacgap")));
    // Bundle ID taken from the cache folder name.
    let generic = f.home.join("Library/Caches/com.example.busy");
    make_dir(&generic, 4096);
    f.sys.bundles.insert("com.example.busy".into());
    let plan = clean::plan(
        &[sel("user-caches", &generic)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(!plan.items[0].blockers.is_empty());
}

#[test]
fn app_started_after_preview_blocks_execution() {
    let mut f = fixture();
    let target = f.home.join("Library/Application Support/Slack/Cache");
    make_dir(&target, 4096);
    let plan = clean::plan(
        &[sel("slack-caches", &target)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(plan.items[0].blockers.is_empty());
    f.sys.bundles.insert("com.tinyspeck.slackmacgap".into());
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Skipped);
    assert!(target.exists());
}

#[test]
fn prefers_official_command_once_per_rule() {
    let mut f = fixture();
    f.sys.commands.insert("brew".into());
    let target = f.home.join("Library/Caches/Homebrew");
    make_dir(&target, 4096);
    let plan = clean::plan(
        &[sel("homebrew-cache", &target)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(matches!(plan.items[0].action, Action::Command { .. }));
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::CommandRan);
    assert_eq!(f.sys.ran.lock().unwrap()[0], vec!["brew", "cleanup", "--prune=all"]);
    assert!(target.exists(), "the command does the cleaning, not OpenDisk");
}

#[test]
fn emptying_trash_needs_explicit_permission() {
    let f = fixture();
    let item = f.home.join(".Trash/old");
    make_dir(&item, 4096);
    let plan = clean::plan(
        &[sel("trash", &item)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(!plan.items[0].blockers.is_empty());
    let opts = CleanOptions {
        allow_permanent: true,
        ..Default::default()
    };
    let plan = clean::plan(&[sel("trash", &item)], rules(), &f.home, &f.sys, &opts);
    assert_eq!(plan.items[0].action, Action::Delete);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Deleted);
}

#[test]
fn user_picked_paths_always_go_to_trash() {
    let f = fixture();
    let picked = f.home.join("old-vm-images");
    make_dir(&picked, 4096);
    let opts = CleanOptions {
        delete_regenerable: true,
        allow_permanent: true,
    };
    let selection = Selection {
        rule_id: None,
        path: picked.clone(),
        bytes: 4096,
    };
    let plan = clean::plan(&[selection], rules(), &f.home, &f.sys, &opts);
    assert_eq!(plan.items[0].action, Action::Trash);
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Trashed);
}
