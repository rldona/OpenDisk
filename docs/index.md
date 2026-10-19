---
layout: home

hero:
  name: OpenDisk
  text: See what fills your disk. Free space safely.
  tagline: An open-source disk cleaner for developers and AI/ML users, and for everyone else too. No telemetry, no accounts, no network.
  image:
    src: /logo.svg
    alt: OpenDisk
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: How it stays safe
      link: /guide/safety
    - theme: alt
      text: View on GitHub
      link: https://github.com/rldona/OpenDisk

features:
  - icon: 🔍
    title: Finds the hidden space
    details: Scans ~/Library and dot-folders like ~/.cache, ~/.npm and ~/.gradle, which Finder doesn't show, and draws them on a disk map.
  - icon: 🧠
    title: Knows dev and AI caches
    details: 61 rules for Xcode, simulators, npm, Gradle, Cargo, pip, uv, conda, Hugging Face, Ollama, GPT4All, LM Studio, Docker, Homebrew and more.
  - icon: 🗑️
    title: Trash first, undo always
    details: Items go to the Trash with an undo journal. Only caches a tool rebuilds on its own can be deleted directly, and only if you ask.
  - icon: 🛡️
    title: Checked twice
    details: Every item is checked against its rule and a hard denylist, and re-checked right before cleaning. Symlinks are never followed.
  - icon: 📏
    title: Honest numbers
    details: Sizes are real allocated blocks with hardlinks counted once, and the space freed is measured on the disk afterwards.
  - icon: 🔒
    title: Private by design
    details: No network access, no telemetry. The desktop app can only call a short list of commands, and everything stays on your Mac.
---

<div style="max-width: 1100px; margin: 48px auto 0;">

![OpenDisk findings](/screenshots/findings.png)

</div>
