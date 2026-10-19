# Cleaning

Cleaning always goes **select → preview → (dry run) → clean → results**.

## Preview

**Preview…** builds a plan: what will happen to each item and what will be skipped, and why.

![Preview](/screenshots/preview.png)

The action for each item is:

| Action | When |
| --- | --- |
| **Move to Trash** | The default for everything. You can put items back. |
| **Delete now** | Only for *regenerable* caches, and only if you ticked **Delete regenerable caches immediately**. Frees the space right away. Also used for items already in the Trash when you confirm emptying it. |
| **Run …** | The rule has an official tool (`brew cleanup`, `docker system prune`…). OpenDisk runs it instead of deleting files. |
| **Skip** | Something is wrong: the app that owns it is running, it is protected, it is a symlink, it changed… The reason is shown. |

## Dry run

**Dry run** performs every check of a real clean and reports what would happen, without changing anything.

![Dry run](/screenshots/dry-run.png)

## Clean

**Clean** re-checks each item right before acting on it. An item is skipped if, since the preview:

- it changed (different folder, or modified),
- it became a symlink, or a folder above it did,
- the app that owns it was opened,
- it is no longer covered by its rule.

## Results

The results screen shows the **space really freed**, measured on the disk before and after, not the sum of the
sizes you selected.

![Results](/screenshots/results.png)

That number can differ from what you selected:

- **Moved to the Trash** frees nothing until you empty the Trash. OpenDisk tells you how much went there.
- **APFS clones and hardlinks** share blocks with other files, so deleting one copy frees less.
- **Local snapshots** can keep deleted blocks for a while. See [APFS snapshots](./snapshots).
- Other apps write to the disk at the same time.

## Undo

Everything moved to the Trash is written to a journal
(`~/Library/Application Support/OpenDisk/journal.jsonl`). **Undo** on the results screen puts the items back where
they were, as long as nothing else took their place. From the command line: `opendisk undo <run id>`.
Finder's **Put Back** works too.

Items that were deleted directly, or cleaned by an official tool, cannot be undone; that is why those are limited to
caches the tools rebuild on their own.
