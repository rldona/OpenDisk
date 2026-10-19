// `npm run demo`: the UI in a normal browser with a made-up home folder
// (demo/data.json) and a stand-in for the Tauri backend (demo/bridge.js).
import { readFileSync } from "node:fs";
import { defineConfig, mergeConfig } from "vite";
import base from "./vite.config";

const read = (file: string) => readFileSync(new URL(file, import.meta.url), "utf8");

export default mergeConfig(
  base,
  defineConfig({
    server: { port: 1430 },
    plugins: [
      {
        name: "opendisk-demo",
        transformIndexHtml: (html) =>
          html.replace(
            "<head>",
            `<head><script>window.__OPENDISK_DEMO__ = ${read("./demo/data.json")};</script><script>${read("./demo/bridge.js")}</script>`,
          ),
      },
    ],
  }),
);
