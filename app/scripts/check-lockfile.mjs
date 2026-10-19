// Fails if the lockfile resolves packages from anywhere but the public npm
// registry (e.g. a private company mirror configured on a developer machine).
// Usage: node check-lockfile.mjs [path/to/package-lock.json] (default: app's).
import { readFileSync } from "node:fs";

const file = process.argv[2] ?? new URL("../package-lock.json", import.meta.url);
const lock = readFileSync(file, "utf8");
const bad = [...lock.matchAll(/"resolved":\s*"([^"]+)"/g)]
  .map((m) => m[1])
  .filter((url) => !url.startsWith("https://registry.npmjs.org/"));
if (/jfrog|artifactory/i.test(lock) || bad.length > 0) {
  console.error(`${file} must only reference https://registry.npmjs.org/`);
  for (const url of bad.slice(0, 10)) console.error("  " + url);
  process.exit(1);
}
console.log(`${file}: public npm registry only`);
