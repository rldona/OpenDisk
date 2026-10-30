import { defineConfig } from "vitepress";

// Published by .github/workflows/pages.yml to https://rldona.github.io/OpenDisk/
export default defineConfig({
  title: "OpenDisk",
  description: "See what fills your disk and free space safely. Open source, no telemetry.",
  base: "/OpenDisk/",
  lang: "en-US",
  cleanUrls: true,
  lastUpdated: true,
  srcExclude: ["assets/**", "scripts/**", "README.md"],
  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: "/OpenDisk/logo.svg" }],
    ["meta", { property: "og:image", content: "https://rldona.github.io/OpenDisk/cover.png" }],
  ],
  themeConfig: {
    logo: "/logo.svg",
    nav: [
      { text: "Guide", link: "/guide/getting-started" },
      { text: "Rules", link: "/reference/rules" },
      { text: "CLI", link: "/reference/cli" },
      { text: "Development", link: "/development/building" },
    ],
    sidebar: [
      {
        text: "Guide",
        items: [
          { text: "Getting started", link: "/guide/getting-started" },
          { text: "Full Disk Access", link: "/guide/full-disk-access" },
          { text: "Scanning and the disk map", link: "/guide/scanning" },
          { text: "Findings", link: "/guide/findings" },
          { text: "Large files", link: "/guide/large-files" },
          { text: "Cleaning", link: "/guide/cleaning" },
          { text: "APFS snapshots", link: "/guide/snapshots" },
          { text: "Safety model", link: "/guide/safety" },
          { text: "FAQ", link: "/guide/faq" },
        ],
      },
      {
        text: "Reference",
        items: [
          { text: "Built-in rules", link: "/reference/rules" },
          { text: "Command line", link: "/reference/cli" },
        ],
      },
      {
        text: "Development",
        items: [
          { text: "Building from source", link: "/development/building" },
          { text: "Writing rules", link: "/development/writing-rules" },
          { text: "Architecture", link: "/development/architecture" },
        ],
      },
    ],
    socialLinks: [{ icon: "github", link: "https://github.com/rldona/OpenDisk" }],
    editLink: {
      pattern: "https://github.com/rldona/OpenDisk/edit/main/docs/:path",
      text: "Edit this page on GitHub",
    },
    search: { provider: "local" },
    footer: {
      message: "Released under the MIT License.",
      copyright: "Copyright © 2026 Raúl López",
    },
  },
});
