// Typed wrappers around the Tauri commands in src-tauri/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Safety = "regenerable" | "review" | "personal";

export interface AppInfo {
  home: string;
  version: string;
  data_volume: string;
  /** `std::env::consts::OS`: "macos", "linux" or "windows". */
  os: string;
  /** `/` on Unix, `\\` on Windows. */
  path_sep: string;
  /** "Trash" or "Recycle Bin". */
  trash_name: string;
  /** "Finder", "Files" or "File Explorer". */
  file_manager: string;
}

export interface DiskInfo {
  total: number;
  available: number;
}

export interface FdaStatus {
  state: "granted" | "denied" | "unknown";
  probed: string | null;
}

export interface ScanProgress {
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  elapsed_ms: number;
  phase: "scanning" | "findings";
}

export interface ScanDone {
  root: string;
  total_bytes: number;
  files: number;
  duration_ms: number;
  cancelled: boolean;
  denied: string[];
  findings_bytes: number;
  error: string | null;
}

export interface TreeNode {
  name: string;
  path: string;
  size: number;
  files: number;
  denied: boolean;
  other: boolean;
  children: TreeNode[];
}

export interface PreferCommand {
  argv: string[];
  description: string;
}

export interface Item {
  path: string;
  bytes: number;
  files: number;
  is_dir: boolean;
  modified: number | null;
  requires_closed: string[];
}

export interface Finding {
  rule_id: string;
  name: string;
  category: string;
  category_label: string;
  safety: Safety;
  explanation: string;
  prefer_command: PreferCommand | null;
  permanent: boolean;
  requires_no_process: string[];
  total_bytes: number;
  items: Item[];
}

export interface Selection {
  rule_id: string | null;
  path: string;
  bytes: number;
}

export interface CleanOptions {
  delete_regenerable: boolean;
  allow_permanent: boolean;
}

export type Action =
  | { kind: "trash" }
  | { kind: "delete" }
  | { kind: "command"; argv: string[]; description: string };

export interface PlanItem {
  rule_id: string | null;
  rule_name: string;
  path: string;
  bytes: number;
  safety: Safety;
  action: Action;
  blockers: string[];
}

export interface Plan {
  id: string;
  items: PlanItem[];
  trash_bytes: number;
  delete_bytes: number;
  command_bytes: number;
  blocked_bytes: number;
}

export type Status =
  | "would_clean"
  | "trashed"
  | "deleted"
  | "erased"
  | "command_ran"
  | "covered_by_command"
  | "skipped"
  | "failed";

export interface ItemOutcome {
  path: string;
  rule_id: string | null;
  action: Action;
  bytes: number;
  status: Status;
  message: string | null;
  trashed_to: string | null;
}

export interface Report {
  run_id: string;
  plan_id: string;
  dry_run: boolean;
  outcomes: ItemOutcome[];
  available_before: number;
  available_after: number;
  freed_bytes: number;
  trashed_bytes: number;
  deleted_bytes: number;
  skipped: number;
  failed: number;
}

export interface Snapshot {
  name: string;
  kind: "time_machine" | "os_update" | "other";
  date: string | null;
}

export interface SnapshotReport {
  snapshots: Snapshot[];
  explanation: string;
  thin_command: string;
  error: string | null;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  diskInfo: () => invoke<DiskInfo>("disk_info"),
  fdaStatus: () => invoke<FdaStatus>("fda_status"),
  openFdaSettings: () => invoke<void>("open_fda_settings"),
  startScan: (target: "home" | "volume") => invoke<void>("start_scan", { target }),
  cancelScan: () => invoke<void>("cancel_scan"),
  getTree: (path?: string, depth = 2) => invoke<TreeNode>("get_tree", { path, depth }),
  getFindings: () => invoke<Finding[]>("get_findings"),
  createPlan: (selection: Selection[], options: CleanOptions) =>
    invoke<Plan>("create_plan", { selection, options }),
  executePlan: (planId: string, dryRun: boolean) =>
    invoke<Report>("execute_plan", { planId, dryRun }),
  undoRun: (runId: string) =>
    invoke<{ restored: string[]; failed: [string, string][] }>("undo_run", { runId }),
  listSnapshots: () => invoke<SnapshotReport>("list_snapshots"),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  onScanProgress: (cb: (p: ScanProgress) => void): Promise<UnlistenFn> =>
    listen<ScanProgress>("scan-progress", (e) => cb(e.payload)),
  onScanDone: (cb: (d: ScanDone) => void): Promise<UnlistenFn> =>
    listen<ScanDone>("scan-done", (e) => cb(e.payload)),
};

export function formatBytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = Math.abs(n);
  let u = 0;
  while (v >= 1000 && u < units.length - 1) {
    v /= 1000;
    u++;
  }
  const s = u === 0 ? `${v}` : v >= 100 ? v.toFixed(0) : v.toFixed(1);
  return `${n < 0 ? "-" : ""}${s} ${units[u]}`;
}

export function tildify(path: string, home: string, sep = "/"): string {
  if (path === home) return "~";
  const prefix = home.endsWith(sep) ? home : home + sep;
  return path.startsWith(prefix) ? "~" + sep + path.slice(prefix.length) : path;
}
