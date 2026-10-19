# Writing rules

Rules live in [`core/rules/macos.yaml`](https://github.com/rldona/OpenDisk/blob/main/core/rules/macos.yaml) and are
compiled into the app. A rule describes one kind of reclaimable data:

```yaml
- id: npm-cache                       # unique, kebab-case
  name: npm cache                     # shown in the app
  category: javascript                # one of the categories below
  safety: regenerable                 # regenerable | review | personal
  paths:                              # globs relative to ~ (one folder level per *)
    - "~/.npm/_cacache"
    - "~/.npm/_npx"
  explanation: >-                     # what it is, why it can go, what happens next time
    Every package tarball npm has downloaded. Projects keep working; npm downloads
    packages again when needed (same as `npm cache clean --force`).
  requires_closed: [com.example.app]  # optional: bundle IDs that must not be running
  requires_no_process: [npm]          # optional: process names that must not be running
  prefer_command:                     # optional: official tool used instead of deleting
    argv: [npm, cache, clean, --force]
    description: Empties the npm cache.
  examples: ["~/.npm/_cacache"]       # required: paths this rule must match (tested)
  non_examples: ["~/.npm/config"]     # optional: paths it must not match (tested)
```

Other fields:

| Field | |
| --- | --- |
| `exclude` | Globs this rule must skip |
| `kind` | `any` (default), `dir` or `file` |
| `min_age_days` | Only items whose modification time is older than this |
| `bundle_id_component` | Index of the path component that is the owning app's bundle ID (e.g. `2` for `~/Library/Caches/<bundle id>`) |
| `priority` | Higher wins when two rules match the same or nested paths (default 50; the catch-all rules use 10) |
| `permanent` | Cleaning deletes for good (only the Trash rule) |

Categories: `ai-models`, `xcode`, `javascript`, `jvm`, `python`, `rust-go`, `containers`, `homebrew`, `app-caches`,
`browsers`, `logs`, `downloads`, `backups`, `trash`.

## Choosing the safety level

- **regenerable**: the owning tool rebuilds it automatically and nothing of value is lost. Keep it for real caches.
- **review**: it can be rebuilt but at a real cost (gigabytes of downloads, archives you may still need).
- **personal**: it is the user's data. Use it whenever in doubt.

## Rules of thumb

- Point at the cache folder itself, not at the app's whole data folder.
- Prefer a tool's own clean-up command when it has one.
- List the apps or processes that write there, so OpenDisk can ask the user to close them.
- Write the explanation for a non-developer: what it is, and what happens next time it is needed.

## Testing

Every rule needs at least one `examples` entry. The test suite builds a fake home folder containing all examples of all
rules and checks that each example is claimed by its own rule (and each `non_examples` entry is not), which also tests
how rules with overlapping paths resolve:

```bash
cargo test -p opendisk-core --test rules_examples
```

The [Built-in rules](/reference/rules) page is generated from the same YAML file.
