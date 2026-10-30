# Getting started

OpenDisk is a desktop app for **macOS 11 or later** (Apple silicon and Intel), **Linux** (Ubuntu 22.04+ or any
distribution through the AppImage) and **Windows 10/11**.

## Install

Every build is on the [releases page](https://github.com/rldona/OpenDisk/releases), and every commit is also available
as an artifact of the [CI workflow](https://github.com/rldona/OpenDisk/actions/workflows/ci.yml) (you need to be signed
in to GitHub).

### macOS

1. Download `OpenDisk_<version>_universal.dmg` from the latest release.
2. Open the `.dmg` and drag **OpenDisk** to **Applications**.
3. The first time, macOS says it cannot check the app for malicious software, because OpenDisk is not yet signed with
   an Apple Developer ID. Open it anyway:
   - **Control-click** OpenDisk in Applications and choose **Open**, then **Open** again, or
   - go to **System Settings › Privacy & Security** and click **Open Anyway** next to the OpenDisk message.

   If you built it yourself or prefer the terminal: `xattr -dr com.apple.quarantine /Applications/OpenDisk.app`.

### Linux

- **Ubuntu 22.04 or later:** download `OpenDisk_<version>_amd64.deb` and run
  `sudo apt install ./OpenDisk_<version>_amd64.deb`.
- **Any distribution:** download the `.AppImage`, make it executable (`chmod +x OpenDisk_*.AppImage`) and run it.
  It needs FUSE 2; on newer distributions install `libfuse2` (or `libfuse2t64`) if it does not start.

### Windows

1. Download `OpenDisk_<version>_x64-setup.exe` (or the `.msi` if the release has one).
2. Run the installer. It installs for the current user only and does not ask for administrator rights.
3. Windows SmartScreen warns because the installer is not signed yet: choose **More info › Run anyway**.

## First scan

1. On macOS, give OpenDisk [Full Disk Access](./full-disk-access) when it asks. Without it macOS hides part of your
   data and the scan under-counts. Linux and Windows need no special permission for the home folder.
2. Choose **Home folder** (or **Whole data volume** / **Whole disk**) and click **Scan**. A typical home folder with a
   few hundred GB takes under a minute.

![Scanning](/screenshots/scanning.png)

3. Look through the [findings](./findings), select what you want to clean, and click **Preview…**.
4. Run a **Dry run** if you want to see what would happen without changing anything, then **Clean**.

Nothing is touched until you confirm the preview, and most things go to the Trash (the Recycle Bin on Windows), where
you can put them back.

## Command line

The same engine is available as a command line tool for scripting and power users. Releases include `opendisk` for
Linux and Windows.
See [Command line](/reference/cli).
