# FAQ

## Is it safe to delete caches?

Caches are, by definition, data a program can rebuild. The cost is time: the next build, install or model load is
slower while the cache fills again. OpenDisk says for each rule what happens next time, and marks the expensive ones
(large model downloads, Xcode archives) as *review*.

## Why does the free space not go up after cleaning?

Probably because the items went to the Trash (the Recycle Bin on Windows): empty it to get the space back. Other reasons are APFS clones and
hardlinks sharing blocks, and local snapshots holding on to deleted data. See [Results](./cleaning#results).

## Why is "System Data" so big in System Settings?

It includes caches, logs, Time Machine local snapshots and app data in `~/Library`. OpenDisk's findings and the
[snapshots tab](./snapshots) explain most of it.

## Does OpenDisk need my password or admin rights?

No. It only works inside your home folder, with your own permissions. It never asks for your password.

## Does it work on Windows or Linux?

Yes. Linux has a `.deb` for Ubuntu 22.04 and later and an `.AppImage` for any distribution; Windows 10/11 has an NSIS
installer (and an `.msi` when WiX is available). Both platforms have their own rule list, and the command line tool is
published for both. On Windows the app uses the Recycle Bin and leaves emptying it to File Explorer.

## Why is the app not in the Mac App Store?

The App Store requires a sandbox that cannot get Full Disk Access, which a disk cleaner needs to be useful.

## How do I report a problem or suggest a rule?

Open an issue on [GitHub](https://github.com/rldona/OpenDisk/issues). For a new rule, include the path, which app
creates it, and how that app behaves when it is gone.
