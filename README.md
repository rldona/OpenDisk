# OpenDisk

OpenDisk is a free, open-source desktop app that shows what fills your disk and
frees space **safely**. It is built for developers and AI/ML users first (Xcode,
simulators, Docker, npm/Gradle/Cargo caches, Hugging Face and local LLM
models), and it stays usable for everyone else.

On a Mac most of the space hides in `~/Library` and in dot-folders such as
`~/.cache`, `~/.npm` or `~/.gradle`, which Finder doesn't show. OpenDisk scans
them, explains each cache it finds (what it is, whether it comes back on its
own, what must be closed first), and cleans only what you pick.

> Status: early MVP, macOS only. Windows and Linux are planned.

## What it does

- **Scans** your home folder (or the whole data volume) in parallel and shows a
  disk map that includes hidden folders.
- **Finds** reclaimable space with 61 declarative rules: Xcode DerivedData,
  device support files and simulators, npm/Yarn/pnpm/Bun, Gradle and Maven,
  Cargo and Go, pip/uv/Poetry/conda, Hugging Face, PyTorch, Ollama, GPT4All and
  LM Studio models, Docker Desktop, Homebrew, Electron app and browser caches,
  logs, old installers in Downloads, iOS backups and the Trash.
- **Cleans** what you select after a preview, and reports the space really
  freed, measured on the disk.
- Lists **APFS local snapshots** and explains why freed space may not show up
  right away.

No network access, no telemetry, no accounts.

## Safety model

Deleting the wrong thing is worse than not freeing space, so OpenDisk is
conservative by design:

- **Trash by default.** Items go to the Trash with the same API Finder uses
  (`NSFileManager trashItemAtURL`), and every action is written to a journal so
  a run can be undone. Only caches marked *regenerable* can be deleted
  directly, and only if you tick "Delete regenerable caches immediately".
- **Allowlist only.** OpenDisk only cleans paths a rule covers (or that you
  picked yourself, which always go to the Trash). A hard denylist protects
  system locations, iCloud Drive, Mail, Messages, Photos libraries, keychains,
  Apple app containers and everything outside your home folder.
- **Re-checked right before acting.** Each item must still match its rule,
  must not be a symlink or sit below one, and must have the same inode and
  modification time as in the preview; otherwise it is skipped.
- **Never follows symlinks, never crosses volumes**, when scanning or deleting.
- **Apps must be closed.** Rules list the bundle IDs and processes that must
  not be running (e.g. Xcode for DerivedData, Slack for its cache).
- **Official tools first** when they exist: `xcrun simctl delete unavailable`,
  `brew cleanup`, `docker system prune`, `go clean -modcache`, `pnpm store prune`.
- **Honest numbers.** Sizes are allocated blocks (`st_blocks`), hardlinks are
  counted once, and after cleaning OpenDisk reports the real `statfs` delta.
  APFS clones and snapshots can make that smaller than the selected size.

Every rule says how safe it is:

| Safety | Meaning | Action |
| --- | --- | --- |
| regenerable | A cache the tool rebuilds on demand | Trash, or delete if you opt in |
| review | Rebuildable but costly (models, archives) | Trash |
| personal | Your own data (backups, Trash) | Trash; emptying the Trash needs an extra confirmation |

### Full Disk Access

macOS hides Mail, Messages, Safari data, the Trash and some app containers from
apps without Full Disk Access, so scans under-count. OpenDisk detects this and
opens **System Settings › Privacy & Security › Full Disk Access** for you. It is
distributed outside the App Store because the App Store sandbox cannot get full
disk access.

## Project layout

```
core/        Rust library: scanner, rule engine (core/rules/*.yaml), cleaner, journal
cli/         `opendisk` command line on the same core
app/         Tauri 2 desktop app (React + TypeScript + Vite in app/src, Rust in app/src-tauri)
```

## Building

Requirements: Rust (stable, via [rustup](https://rustup.rs)), Node.js 20+, and
Xcode command line tools on macOS.

```bash
# Core and CLI
cargo test
cargo run -p opendisk-cli -- findings
cargo run -p opendisk-cli -- clean            # dry run (default)
cargo run -p opendisk-cli -- clean --execute  # really clean, asks first

# Desktop app
cd app
npm ci
npm run tauri dev       # development
npm run tauri build     # .app and .dmg in target/release/bundle
```

Local builds are ad-hoc signed (`signingIdentity: "-"` in
`app/src-tauri/tauri.conf.json`), so they never pick up a developer
certificate from your keychain. Release signing and notarization will be added
to CI later.

The frontend's `.npmrc` pins the public npm registry, and CI fails if
`package-lock.json` references any other registry.

### CLI

```
opendisk scan [PATH]         where the space goes (default: home folder)
opendisk findings            what the rules found, grouped by category
opendisk clean               dry run of cleaning every regenerable finding
  --rule ID                  only these rules (see `opendisk rules`)
  --safety review            only these safety levels
  --execute                  really clean (asks for confirmation; --yes to skip)
  --delete-regenerable       delete regenerable caches instead of trashing
opendisk undo RUN_ID         put back what a run moved to the Trash
opendisk snapshots           APFS local snapshots
opendisk fda [--open]        Full Disk Access status
```

`--home DIR` points every command at another folder, which is how the tests run
against fixture trees.

## Writing rules

Rules live in `core/rules/macos.yaml` and are embedded in the binary. Each rule
has an id, a category, a safety level, path globs relative to `~`, a
human-readable explanation, the apps that must be closed, an optional official
command, and `examples` that the test suite materialises in a fake home folder
and checks are claimed by that rule. Specific rules win over the catch-all ones
(`~/Library/Caches/*`, `~/.cache/*`) through `priority`.

## License

[MIT](LICENSE) © Raúl López
