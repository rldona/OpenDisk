# Building from source

## Requirements

- macOS with the Xcode command line tools (`xcode-select --install`)
- [Rust](https://rustup.rs) stable
- Node.js 22 or later

## Engine and command line

```bash
cargo test                          # core and CLI tests
cargo run -p opendisk-cli -- findings
cargo run -p opendisk-cli -- clean  # dry run
```

`cargo test` never touches your real files: every test builds its own folder tree in a temporary directory and uses a
fake Trash.

## Desktop app

```bash
cd app
npm ci
npm run tauri dev      # development, with hot reload
npm run tauri build    # .app and .dmg in target/release/bundle
```

Local builds are ad-hoc signed (`"signingIdentity": "-"` in `app/src-tauri/tauri.conf.json`), so they never use a
developer certificate from your keychain by accident.

## UI demo mode

```bash
cd app
npm run demo           # http://localhost:1430
```

Runs the interface in a normal browser with a made-up home folder (`app/demo/data.json`) and a stand-in for the Rust
backend (`app/demo/bridge.js`). It is handy for working on the UI without building Rust, and it is what the manual's
screenshots use. Add `?fda=denied` to the URL to see the Full Disk Access banner.

## Manual and screenshots

```bash
cd docs
npm ci
npm run dev            # this manual on http://localhost:5173/OpenDisk/
npm run screenshots    # with `npm run demo` running: refreshes the screenshots and the README cover
```

`npm run screenshots` drives the demo with your installed Google Chrome (set `CHROME=/path/to/chrome` to use another).

## npm registry

`app/.npmrc` and `docs/.npmrc` pin the public npm registry, and CI fails if a lockfile references any other
registry. If your machine uses a private mirror, the pinned registry keeps it out of the lockfiles.

## Continuous integration

`.github/workflows/ci.yml` runs on every push and pull request:

- `cargo fmt`, `clippy` and the tests on macOS, Ubuntu 22.04 and Windows;
- the lockfile check, type checking and the frontend build;
- a universal (Apple silicon + Intel) `.app` and `.dmg` (`OpenDisk-macos-universal`);
- a `.deb` and an `.AppImage` (`OpenDisk-linux-amd64-deb`, `OpenDisk-linux-amd64-appimage`);
- an NSIS installer and, best effort, an `.msi` (`OpenDisk-windows-x64-nsis`, `OpenDisk-windows-x64-msi`);
- the `opendisk` command line binary for Linux and Windows (`OpenDisk-cli-*`).

The Windows job also runs the real Recycle Bin round trip (`cargo test -- --ignored real_recycle_bin`), which macOS and
Linux cannot exercise.

Pushing a tag such as `v0.2.0` publishes a GitHub release with every installer, both CLI binaries and a
`SHA256SUMS.txt`. MSI versions must be `major.minor.patch` without a prerelease suffix; if WiX rejects the tag the
`.msi` is skipped and the NSIS installer still ships.

`.github/workflows/pages.yml` builds this manual and publishes it to GitHub Pages.
