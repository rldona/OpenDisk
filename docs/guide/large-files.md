# Large files

The **Findings** tab only shows what the rules recognise: caches, models, installers, backups. The **Large files** tab
answers a different question: *which single files are taking the most space, anywhere on this disk?*


## What it lists

Every regular file at or above the size you choose, sorted largest first. The threshold can be 100 MB, 500 MB, 1 GB or
5 GB; the default is 500 MB. Sizes are **allocated** bytes, the same number `du` reports, so sparse files and hardlinks
are not over-counted. Hardlinks are listed once.

The list covers the folder you scanned:

- **Home folder**: your files, all cleanable.
- **Whole data volume / whole disk**: every user and system folder that can be read. Files outside your home folder are
  shown with a **protected** note and cannot be selected.

OpenDisk never cleans outside your home folder. That is deliberate: other users' files, system files and external
volumes need permissions and a safety model that this release does not provide. You can still use the list to see what
fills the disk and reveal the file in Finder, Files or File Explorer.

## Cleaning

Select the files you want (only cleanable ones have a checkbox), then **Preview…**. The preview works exactly like a
normal cleaning: every item is checked again, must still exist and be unchanged, and goes to the Trash or Recycle Bin.
Undo puts it back from there.

Files picked here have no rule, so OpenDisk always moves them to the Trash; it never deletes them permanently. Emptying
the Trash is still your decision.

## Command line

```bash
opendisk large --min 1GB
opendisk large / --min 5GB --json
opendisk large --min 2GB --clean --execute --yes
```

See [Command line](/reference/cli#opendisk-large-path) for every option.
