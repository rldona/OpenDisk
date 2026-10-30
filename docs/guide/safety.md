# Safety model

Deleting the wrong thing is worse than not freeing space, so OpenDisk is conservative by design. These rules are
enforced by the cleaning engine itself, the same one the app and the command line use, and they are covered by tests
that run against fake folder trees.

## Trash by default

Items go to the Trash (the Recycle Bin on Windows) through the same system call the file manager uses, and each action is written to a journal so a run can be
[undone](./cleaning#undo). Only caches marked *regenerable* can be deleted directly, and only if you opt in.

| Safety | Meaning | What OpenDisk does |
| --- | --- | --- |
| <Badge type="tip" text="regenerable" /> | A cache the owning tool rebuilds on demand | Trash, or delete if you tick *Delete regenerable caches immediately* |
| <Badge type="warning" text="review" /> | Rebuildable but costly (models, archives, emulators) | Trash |
| <Badge type="danger" text="personal" /> | Your own data (device backups, Docker volumes, the Trash) | Trash; emptying the Trash needs an extra confirmation (macOS and Linux) |

## Only what a rule covers

OpenDisk only cleans paths that one of its [rules](/reference/rules) covers, or that you picked yourself (those always
go to the Trash). Each rule explains why its paths are safe to remove. On Windows, an item bigger than the Recycle Bin is blocked in the preview instead of letting Windows delete it permanently, and emptying the Recycle Bin is left to File Explorer.

On top of that, a **hard denylist** refuses, whatever is selected:

- anything outside your home folder (`/System`, `/usr`, `/Library`, `/Applications`, other volumes…);
- your home folder itself and its main folders (`~/Library`, `~/Documents`, `~/Downloads`, `~/Library/Caches`…);
- iCloud Drive and cloud storage folders, Mail, Messages, Photos libraries, keychains, Safari, preferences,
  `~/.ssh`, `~/.gnupg`, `~/.config`;
- the containers of Apple's own apps.

## Checked again right before acting

When you preview, OpenDisk records each item's identity (device, inode, type, modification time). Right before
acting it checks again that the item:

- still matches its rule and is not denylisted,
- is not a symlink, and no folder above it is a symlink,
- is the same item with the same modification time as in the preview,
- is not owned by an app or process that is running.

If anything differs, the item is skipped and the reason is shown.

## Never follows symlinks, never crosses volumes

When scanning, matching rules and deleting, OpenDisk works on links themselves and never on what they point to, and
it stops at the boundary of another disk or volume. A symlink inside a cache folder that points at your documents is
removed as a link; the documents are untouched.

## Apps must be closed

Rules list the apps (by bundle ID) and processes that must not be running, for example Xcode for DerivedData, Slack
for its cache or `ollama` for its models. For per-app caches named after the app (`~/Library/Caches/com.example.app`),
the app is detected from the folder name.

## Official tools first

When a tool has its own way to clean up, OpenDisk uses it instead of deleting files: `xcrun simctl delete unavailable`,
`brew cleanup --prune=all`, `docker system prune --force`, `go clean -modcache`, `pnpm store prune`,
`conda clean --all`. They run directly, never through a shell.

## Honest numbers

Sizes are allocated blocks, hardlinks are counted once, and after cleaning OpenDisk reports the real change in free
space. See [Results](./cleaning#results) for why that can differ from the selected size.

## Private

OpenDisk has no network access and no telemetry. The desktop app's interface can only call a fixed list of commands
(scan, list findings, preview, clean, undo…); it has no general file or shell access.
