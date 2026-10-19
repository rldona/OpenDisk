// Loads the built-in rules straight from core/rules/macos.yaml at build time,
// so the manual always lists exactly what the app ships.
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

const file = new URL("../../core/rules/macos.yaml", import.meta.url);

export default {
  watch: ["../../core/rules/macos.yaml"],
  load(): Rule[] {
    return parse(readFileSync(file, "utf8")).rules as Rule[];
  },
};
