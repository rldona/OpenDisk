# Command line

`opendisk` uses the same engine as the app. Cleaning is a **dry run** unless you pass `--execute`.

Build it from source with `cargo build --release -p opendisk-cli`; the binary is `target/release/opendisk`.

## Commands

### `opendisk scan [PATH]`

Shows where the space goes. `PATH` defaults to your home folder; use the data volume root for the whole disk
(`/System/Volumes/Data` on macOS, `/` on Linux, `C:\` on Windows).

| Option | |
| --- | --- |
| `--depth N` | Levels of folders to print, and to include with `--json` (default 2) |
| `--top N` | Children shown per folder (default 10) |
| `--json` | Machine-readable output |
| `--no-index` | Don't save the scan in the local scan history |

### `opendisk findings`

Lists what the rules found, grouped by category, largest first.

| Option | |
| --- | --- |
| `--min-size SIZE` | Hide items smaller than this, e.g. `100MB` (default `1MB`) |
| `--all` | Show every matched path, not only the top 5 per rule |
| `--json` | Machine-readable output |

### `opendisk large [PATH]`

Lists the individual files above a size, anywhere under `PATH`, largest first. It does not depend on the rules: it is
the way to find that one huge video, disk image or archive that no cache rule knows about. `PATH` defaults to the home
folder; use the data volume root (`/System/Volumes/Data`, `/` or `C:\`) to see the whole disk.

Files outside your home folder are listed but marked `protected`: OpenDisk can show and reveal them, but only cleans
inside your home folder.

| Option | |
| --- | --- |
| `--min SIZE` | Minimum allocated size, e.g. `500MB` (default `500MB`) |
| `--clean` | Plan the cleanable files and print the preview. Dry run unless `--execute` |
| `--execute` | Really clean the selected large files (with `--clean`) |
| `--yes`, `-y` | Don't ask for confirmation |
| `--json` | Machine-readable list (or the cleaning report with `--clean`) |

```bash
opendisk large --min 1GB                          # biggest files in your home
opendisk large / --min 5GB                        # whole disk, read only
opendisk large --min 2GB --clean --execute --yes  # trash every cleanable file >= 2 GB
```

### `opendisk clean`

Plans a clean, prints it, and runs every safety check. Without `--execute` nothing is changed.
With no `--rule` or `--safety`, it selects every *regenerable* finding.

| Option | |
| --- | --- |
| `--rule ID` | Only these rules (repeatable; see `opendisk rules`) |
| `--safety LEVEL` | Only these safety levels: `regenerable`, `review`, `personal` (repeatable) |
| `--min-size SIZE` | Skip items smaller than this (default `1MB`) |
| `--execute` | Really clean. Asks for confirmation |
| `--yes`, `-y` | Don't ask for confirmation (needed without a terminal) |
| `--delete-regenerable` | Delete regenerable caches instead of moving them to the Trash |
| `--allow-permanent` | Allow emptying the Trash (macOS and Linux; on Windows empty the Recycle Bin from Explorer) |
| `--json` | Print the report as JSON |

```bash
opendisk clean --rule npm-cache --rule pip-cache                 # dry run
opendisk clean --rule npm-cache --execute --delete-regenerable  # frees the space now
```

After a real clean it prints the space really freed and the run id for undo.

### `opendisk undo RUN_ID`

Puts back everything that run moved to the Trash or Recycle Bin.

### `opendisk rules`

Lists the built-in rules: id, safety, category and name. Details are in [Built-in rules](./rules).

### `opendisk snapshots`

Lists APFS local snapshots with an explanation (macOS only; hidden elsewhere).

### `opendisk fda [--open]`

Shows whether the terminal has Full Disk Access; `--open` opens the settings pane.

## Global options

`--home DIR` makes every command treat `DIR` as the home folder: rules, denylist, journal and scan history all follow
it (the default comes from `$HOME` or `%USERPROFILE%`). This is how the test suite runs against fake folder trees, and a
safe way to try OpenDisk on a copy.
