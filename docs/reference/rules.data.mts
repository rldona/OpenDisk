// Loads the built-in rules straight from core/rules/*.yaml at build time, so
// the manual always lists exactly what each platform ships.
import { readFileSync } from "node:fs";
import { parse } from "yaml";

export interface Rule {
  id: string;
  name: string;
  category: string;
  safety: "regenerable" | "review" | "personal";
  explanation: string;
  paths: string[];
  requires_closed?: string[];
  requires_no_process?: string[];
  prefer_command?: { argv: string[]; description: string };
  min_age_days?: number;
  permanent?: boolean;
}

function load(file: string): Rule[] {
  return parse(readFileSync(new URL(`../../core/rules/${file}`, import.meta.url), "utf8")).rules as Rule[];
}

export default {
  watch: [
    "../../core/rules/macos.yaml",
    "../../core/rules/linux.yaml",
    "../../core/rules/windows.yaml",
  ],
  load(): Record<"macos" | "linux" | "windows", Rule[]> {
    return {
      macos: load("macos.yaml"),
      linux: load("linux.yaml"),
      windows: load("windows.yaml"),
    };
  },
};
