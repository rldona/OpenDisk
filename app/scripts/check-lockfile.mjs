// Fails if the lockfile resolves packages from anywhere but the public npm
// registry (e.g. a private company mirror configured on a developer machine).
import { readFileSync } from "node:fs";

const lock = readFileSync(new URL("../package-lock.json", import.meta.url), "utf8");
const bad = [...lock.matchAll(/"resolved":\s*"([^"]+)"/g)]
  .map((m) => m[1])
  .filter((url) => !url.startsWith("https://registry.npmjs.org/"));
if (/jfrog|artifactory/i.test(lock) || bad.length > 0) {
  console.error("package-lock.json must only reference https://registry.npmjs.org/");
  for (const url of bad.slice(0, 10)) console.error("  " + url);
  process.exit(1);
}
console.log("lockfile OK: public npm registry only");
