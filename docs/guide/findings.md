# Findings

After a scan OpenDisk runs its [rules](/reference/rules) and lists what they matched, grouped by category and sorted
by size.

![Findings](/screenshots/findings.png)

Each finding shows:

- **What it is and why it can go**, in plain words, including what happens next time you need it.
- A **safety badge**:
  - <Badge type="tip" text="Regenerable" /> a cache the tool rebuilds on its own;
  - <Badge type="warning" text="Review" /> rebuildable but costly to get back (large model downloads, build archives);
  - <Badge type="danger" text="Personal" /> your own data, such as device backups.
- The **official tool** OpenDisk will use instead of deleting files, when there is one (`brew cleanup`,
  `xcrun simctl delete unavailable`, `docker system prune`…).
- The **apps that must be closed** before cleaning it.

Click a finding to see every matched folder or file, its size, and **Show in Finder**.

## Selecting

Tick a whole finding or single items. The bar at the bottom shows how much you selected and opens the
[preview](./cleaning).

![Selecting items](/screenshots/selection.png)

## Why something is not listed

- No rule covers it. OpenDisk only offers what a rule explains; use the [disk map](./scanning#the-disk-map) to find
  everything else, and [suggest a rule](/development/writing-rules).
- It is empty. Empty matches are hidden.
- A more specific rule already claimed it. For example `~/.cache/huggingface/hub` belongs to the Hugging Face rule,
  not to the generic `~/.cache` rule.
- It is a symlink, or lives under a protected folder (see [Safety model](./safety)).


## Files the rules do not know

Findings only explain paths that a rule recognises. If the disk is full and none of the findings explains it, open the
[Large files](./large-files) tab: it lists every individual file above the size you choose, with no rule required.
