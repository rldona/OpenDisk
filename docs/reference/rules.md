---
outline: [2, 3]
---

<script setup>
import { data as rules } from "./rules.data.mts";
const categories = [
  ["ai-models", "AI models"], ["xcode", "Xcode & simulators"], ["javascript", "JavaScript"], ["jvm", "Java & Android"],
  ["python", "Python"], ["rust-go", "Rust & Go"], ["containers", "Containers"], ["homebrew", "Homebrew"],
  ["app-caches", "App caches"], ["browsers", "Browsers"], ["logs", "Logs"], ["downloads", "Downloads"],
  ["backups", "Device backups"], ["trash", "Trash"],
];
</script>

# Built-in rules

OpenDisk ships **{{ rules.length }} rules** for macOS. This page is generated from
[`core/rules/macos.yaml`](https://github.com/rldona/OpenDisk/blob/main/core/rules/macos.yaml),
the same file that is compiled into the app, so it is always up to date.

Paths are relative to your home folder (`~`). `*` matches anything inside one folder level and
`{a,b}` matches either name. See [Safety model](/guide/safety) for what each safety level means.

<template v-for="[cat, label] in categories" :key="cat">
  <h2 :id="cat">{{ label }}</h2>
  <div v-for="r in rules.filter((r) => r.category === cat)" :key="r.id" class="rule">
    <h3 :id="r.id">{{ r.name }} <Badge :type="r.safety === 'regenerable' ? 'tip' : r.safety === 'review' ? 'warning' : 'danger'" :text="r.safety" /><Badge v-if="r.permanent" type="danger" text="permanent" /></h3>
    <p>{{ r.explanation }}</p>
    <ul>
      <li>Rule id: <code>{{ r.id }}</code></li>
      <li>Paths: <code v-for="p in r.paths" :key="p" style="margin-right: 6px">{{ p }}</code></li>
      <li v-if="r.requires_closed?.length">Apps that must be closed: <code v-for="b in r.requires_closed" :key="b" style="margin-right: 6px">{{ b }}</code></li>
      <li v-if="r.requires_no_process?.length">Processes that must not run: <code v-for="p in r.requires_no_process" :key="p" style="margin-right: 6px">{{ p }}</code></li>
      <li v-if="r.prefer_command">Cleans with <code>{{ r.prefer_command.argv.join(" ") }}</code>: {{ r.prefer_command.description }}</li>
      <li v-if="r.min_age_days">Only items older than {{ r.min_age_days }} days</li>
    </ul>
  </div>
</template>
