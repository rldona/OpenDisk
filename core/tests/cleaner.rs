//! Cleaner tests against fixture trees in temp dirs, with a fake system so the
//! real Trash, real apps and real commands are never touched.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use opendisk_core::clean::{self, Action, CleanOptions, Selection, Status};
use opendisk_core::journal::Journal;
use opendisk_core::paths::canonicalize;
use opendisk_core::rules::RuleSet;
use opendisk_core::system::{System, TrashCapacity, TrashOutcome};

struct FakeSystem {
    trash_dir: PathBuf,
    bundles: HashSet<String>,
    processes: HashSet<String>,
    commands: HashSet<String>,
    ran: Mutex<Vec<Vec<String>>>,
    capacity: TrashCapacity,
    erase: bool,
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
            capacity: TrashCapacity::Unlimited,
            erase: false,
        }
    }
}

impl System for FakeSystem {
    fn trash(&self, path: &Path) -> io::Result<TrashOutcome> {
        if self.erase {
            fs::remove_dir_all(path).or_else(|_| fs::remove_file(path))?;
            return Ok(TrashOutcome::Erased);
        }
        let to = self.trash_dir.join(path.file_name().unwrap());
        fs::rename(path, &to)?;
        Ok(TrashOutcome::Moved(Some(to)))
    }
    fn trash_capacity(&self, _path: &Path) -> TrashCapacity {
        self.capacity.clone()
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
    let root = canonicalize(tmp.path()).unwrap();
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

/// Directory link: symlink on Unix, junction on Windows (no admin needed).
fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        // mklink treats `/` as a switch, so use native separators.
        let native = |p: &Path| p.to_string_lossy().replace('/', "\\");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(native(link))
            .arg(native(target))
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J {link:?} -> {target:?}");
    }
}

fn sel(rule: &str, path: &Path) -> Selection {
    Selection {
        rule_id: Some(rule.into()),
        path: path.to_path_buf(),
        bytes: 1,
    }
}

fn rules() -> &'static RuleSet {
    RuleSet::macos()
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(target.join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
    }
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
    link_dir(&outside, &link);
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
    link_dir(&outside, &f.home.join(".npm"));
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
    link_dir(&outside, &target.join("evil-link"));
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
    link_dir(&outside, &target);
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

fn with_bytes(rule: &str, path: &Path, bytes: u64) -> Selection {
    Selection {
        rule_id: Some(rule.into()),
        path: path.to_path_buf(),
        bytes,
    }
}

#[test]
fn blocks_items_too_big_for_the_trash() {
    let mut f = fixture();
    f.sys.capacity = TrashCapacity::MaxBytes(1_000);
    let target = f.home.join("Library/Caches/com.example.app");
    make_dir(&target, 4096);
    let plan = clean::plan(
        &[with_bytes("user-caches", &target, 2_000)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    assert!(
        plan.items[0].blockers.iter().any(|b| b.contains("Too big")),
        "{:?}",
        plan.items[0].blockers
    );
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Skipped);
    assert!(target.exists());
}

#[test]
fn blocks_when_the_trash_is_disabled_or_unknown() {
    for capacity in [
        TrashCapacity::Disabled,
        TrashCapacity::Unknown("registry unreadable".into()),
    ] {
        let mut f = fixture();
        f.sys.capacity = capacity;
        let target = f.home.join("Library/Caches/com.example.app");
        make_dir(&target, 4096);
        let plan = clean::plan(
            &[with_bytes("user-caches", &target, 10)],
            rules(),
            &f.home,
            &f.sys,
            &CleanOptions::default(),
        );
        assert!(!plan.items[0].blockers.is_empty(), "{:?}", plan.items[0].blockers);
    }
}

#[test]
fn erased_items_are_reported_honestly() {
    let mut f = fixture();
    f.sys.erase = true;
    let target = f.home.join("Library/Caches/com.example.app");
    make_dir(&target, 4096);
    let plan = clean::plan(
        &[sel("user-caches", &target)],
        rules(),
        &f.home,
        &f.sys,
        &CleanOptions::default(),
    );
    let report = clean::execute(&plan, rules(), &f.sys, Some(&f.journal), false).unwrap();
    assert_eq!(report.outcomes[0].status, Status::Erased);
    assert_eq!(report.deleted_bytes, 1);
    assert_eq!(report.trashed_bytes, 0);
    assert!(!target.exists());
    let entries = f.journal.entries().unwrap();
    assert_eq!(entries[0].action, opendisk_core::journal::JournalAction::Deleted);
}

#[test]
fn undo_reports_items_whose_trash_location_is_unknown() {
    use opendisk_core::journal::{JournalAction, JournalEntry};
    let f = fixture();
    let path = f.home.join(".npm/_cacache");
    f.journal
        .append(&JournalEntry {
            run_id: "run-1".into(),
            at: 1,
            action: JournalAction::Trashed,
            rule_id: Some("npm-cache".into()),
            path: path.clone(),
            trashed_to: None,
            bytes: 1,
            detail: None,
        })
        .unwrap();
    let (restored, failed) = f.journal.undo("run-1").unwrap();
    assert!(restored.is_empty());
    assert_eq!(failed.len(), 1);
    assert!(failed[0].1.contains("Recycle Bin") || failed[0].1.contains("Trash"));
}

/// Real Windows Recycle Bin round trip. Ignored by default because it touches
/// the machine's Recycle Bin; the Windows CI job runs it with `--ignored`.
#[cfg(windows)]
#[test]
#[ignore = "uses the real Recycle Bin; run on Windows CI"]
fn real_recycle_bin_roundtrip() {
    use opendisk_core::system::{RealSystem, TrashOutcome};

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("opendisk-recycle-test");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("a.txt"), b"x").unwrap();

    let sys = RealSystem;
    let outcome = sys.trash(&path).unwrap();
    assert!(!path.exists(), "the item left its original place: {outcome:?}");
    let TrashOutcome::Moved(Some(trashed)) = outcome else {
        panic!("expected the Recycle Bin to report where the item went");
    };
    assert!(trashed.exists(), "the $R entry exists: {}", trashed.display());

    opendisk_core::system::restore_trashed(&trashed, &path).unwrap();
    assert!(path.join("a.txt").exists(), "Undo put the folder back");
}
