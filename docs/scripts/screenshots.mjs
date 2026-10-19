// Regenerates the manual's screenshots (docs/public/screenshots) and the
// repository cover (docs/assets/cover.png) from the UI's demo mode, which uses
// a made-up home folder, so no real files appear.
//
//   1. cd app && npm run demo          (serves the demo on http://localhost:1430)
//   2. cd docs && npm run screenshots  (uses your installed Google Chrome; set CHROME=... to override)
import { readFileSync, writeFileSync } from "node:fs";
import puppeteer from "puppeteer-core";

const base = process.argv[2] ?? "http://localhost:1430";
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const out = (name) => new URL(`../public/screenshots/${name}.png`, import.meta.url).pathname;
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await puppeteer.launch({ executablePath: chrome, headless: true });
const page = await browser.newPage();
await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 2 });
await page.emulateMediaFeatures([{ name: "prefers-color-scheme", value: "dark" }]);

const clickText = async (text, selector = "button") => {
  const ok = await page.evaluate(
    (text, selector) => {
      const el = [...document.querySelectorAll(selector)].find((e) => e.textContent.includes(text));
      el?.click();
      return Boolean(el);
    },
    text,
    selector,
  );
  if (!ok) throw new Error(`no ${selector} containing "${text}"`);
  await wait(400);
};
const shot = async (name) => {
  await page.screenshot({ path: out(name) });
  console.log(`wrote public/screenshots/${name}.png`);
};

await page.goto(`${base}/?fda=denied`, { waitUntil: "networkidle0" });
await shot("onboarding");

await page.goto(base, { waitUntil: "networkidle0" });
await clickText("Scan home folder");
await wait(1200);
await shot("scanning");
await page.waitForSelector(".findings", { timeout: 10_000 });
await wait(300);
await shot("findings");
const appShot = await page.screenshot({ encoding: "base64" });

// Clicked through the DOM: the selection bar that appears can cover checkboxes.
for (const name of ["npm cache", "Xcode DerivedData", "Google Chrome caches", "Gradle caches"]) {
  await page.$eval(`input[aria-label="Select ${name}"]`, (el) => el.click());
}
await clickText("npm cache", ".finding-title");
await page.evaluate(() => document.querySelector(".has-selection")?.scrollIntoView({ block: "center" }));
await wait(300);
await clickText("Delete regenerable caches immediately", "label");
await shot("selection");
await clickText("Preview");
await shot("preview");
await clickText("Dry run");
await shot("dry-run");
await clickText("Clean ");
await shot("results");
await clickText("Close");
await page.evaluate(() => window.scrollTo(0, 0));
await clickText("Disk map");
await wait(500);
await shot("disk-map");
await clickText("Snapshots");
await wait(300);
await shot("snapshots");

// The cover: the findings screenshot laid out in assets/cover.html at 1280x640.
const cover = await browser.newPage();
await cover.setViewport({ width: 1280, height: 640, deviceScaleFactor: 1 });
await cover.setContent(readFileSync(new URL("../assets/cover.html", import.meta.url), "utf8"));
await cover.evaluate(async (src) => {
  const img = document.getElementById("app");
  img.src = `data:image/png;base64,${src}`;
  await img.decode();
}, appShot);
writeFileSync(new URL("../assets/cover.png", import.meta.url), await cover.screenshot());
console.log("wrote assets/cover.png");
await browser.close();
