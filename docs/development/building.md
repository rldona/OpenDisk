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

- `cargo fmt`, `clippy` and the tests on macOS;
- the lockfile check, type checking and the frontend build;
- a universal (Apple silicon + Intel) `.app` and `.dmg`, uploaded as the `OpenDisk-macos-universal` artifact.

Pushing a tag such as `v0.1.0` also publishes a GitHub release with the `.dmg` attached.

`.github/workflows/pages.yml` builds this manual and publishes it to GitHub Pages.
