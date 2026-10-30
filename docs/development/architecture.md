# Architecture

```
core/        Rust library: the engine
  src/scan.rs        parallel walk, allocated sizes, hardlinks once, no symlinks, no volume crossing
  src/rules.rs       YAML rule loading, validation and glob matching
  rules/macos.yaml   the built-in rules
  src/findings.rs    expands rules against a home folder and sizes the matches
  src/clean.rs       plan → execute → verify, with re-checks before each action
  src/journal.rs     append-only journal and undo
  src/paths.rs       the hard denylist
  src/system.rs      Trash, running apps, free space, commands (behind a trait, faked in tests)
  src/fda.rs         Full Disk Access detection
  src/snapshots.rs   APFS local snapshots
  src/index.rs       SQLite scan history
cli/         `opendisk` command line on the same engine
app/         Tauri 2 desktop app
  src-tauri/         Rust commands exposed to the UI (an explicit allowlist)
  src/               React + TypeScript interface
  demo/              demo data and backend stand-in for screenshots and UI work
docs/        this manual (VitePress)
```

## Why these choices

- **Rust engine as a library.** The scanner and cleaner are the parts that must be fast and correct, so they live in
  one tested crate used by both the app and the command line.
- **Tauri instead of Electron.** The heavy logic is Rust anyway; Tauri gives a ~10 MB app instead of ~150 MB, and its
  command allowlist limits what the interface can ask the engine to do.
- **Declarative rules.** Knowledge about what is safe to delete is data, reviewed and tested like code, not logic
  scattered through the program.
- **Distributed outside the App Store.** The App Store sandbox cannot get Full Disk Access.

## Cleaning flow

1. `findings::find` expands every rule's globs under the home folder, without following symlinks, resolves overlaps by
   priority and sizes each match (reusing the scan tree when there is one).
2. `clean::plan` validates the selection against the rules and the denylist, records a fingerprint (device, inode,
   type, modification time) of each path, chooses the action and lists blockers such as running apps.
3. `clean::execute` re-checks each item, acts (Trash, delete or the rule's command) or only reports in a dry run,
   writes the journal, and measures free space before and after with `statfs`.

## Not done yet

- A faster macOS listing with `getattrlistbulk` (the walker uses portable `readdir` for now).
- Deleting through directory file descriptors (`openat`/`unlinkat`) to close the last check-then-act window.
- Showing what grew since the last scan, from the scan history.
- Developer ID and Authenticode signing, and notarization.
- Emptying the Windows Recycle Bin from inside the app.
- A faster Windows scan that asks for size and file id per directory instead of per file.
