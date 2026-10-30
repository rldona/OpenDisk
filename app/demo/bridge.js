// Demo mode: stands in for the Tauri backend so the UI runs in a normal browser
// with the made-up home folder in data.json. Used for screenshots, the manual
// and UI work without building Rust. Never part of the real app (see
// vite.demo.config.ts). `?fda=denied` shows the Full Disk Access banner.
(() => {
  const D = window.__OPENDISK_DEMO__;
  const params = new URLSearchParams(location.search);
  const callbacks = new Map();
  const listeners = {};
  let nextId = 1;
  const emit = (event, payload) =>
    (listeners[event] || []).forEach((id) => callbacks.get(id)?.({ event, id: 0, payload }));

  const root = D.scan.root;
  const find = (path) => {
    let node = D.scan.tree;
    if (path === root) return node;
    for (const seg of path.slice(root.length + 1).split("/")) {
      node = node.children.find((c) => c.name === seg);
      if (!node) return null;
    }
    return node;
  };
  const toDto = (n, path, depth) => {
    const children = depth > 0 ? n.children.slice(0, 40).map((c) => toDto(c, `${path}/${c.name}`, depth - 1)) : [];
    if (depth > 0 && n.own_files_size > 0 && n.children.length > 0) {
      children.push({ name: "Files", path, size: n.own_files_size, files: 0, denied: false, other: true, children: [] });
    }
    return { name: n.name, path, size: n.size, files: n.files, denied: n.denied, other: false, children };
  };

  let findings = structuredClone(D.findings);
  let available = D.disk.available;
  const plans = {};
  const running = new Set(["com.google.Chrome"]);

  const handlers = {
    app_info: () => ({
      home: root,
      version: "demo",
      data_volume: "/System/Volumes/Data",
      os: "macos",
      path_sep: "/",
      trash_name: "Trash",
      file_manager: "Finder",
    }),
    disk_info: () => ({ total: D.disk.total, available }),
    fda_status: () => ({ state: params.get("fda") === "denied" ? "denied" : "granted", probed: null }),
    open_fda_settings: () => null,
    start_scan: () => {
      let tick = 0;
      const timer = setInterval(() => {
        tick += 1;
        const f = tick / 12;
        emit("scan-progress", {
          files: Math.round(D.scan.stats.files * f),
          dirs: Math.round(D.scan.stats.dirs * f),
          bytes: Math.round(D.scan.tree.size * f),
          errors: 0,
          elapsed_ms: Math.round(D.scan.duration_ms * f),
          phase: tick > 10 ? "findings" : "scanning",
        });
        if (tick === 12) {
          clearInterval(timer);
          emit("scan-done", {
            root,
            total_bytes: D.scan.tree.size,
            files: D.scan.tree.files,
            duration_ms: D.scan.duration_ms,
            cancelled: false,
            denied: [],
            findings_bytes: findings.reduce((s, f) => s + f.total_bytes, 0),
            error: null,
          });
        }
      }, 200);
    },
    cancel_scan: () => null,
    get_tree: ({ path, depth }) => {
      const p = path || root;
      const node = find(p);
      if (!node) throw "folder not in the scan";
      return toDto(node, p, depth ?? 2);
    },
    get_findings: () => findings,
    create_plan: ({ selection, options }) => {
      const byRule = Object.fromEntries(findings.map((f) => [f.rule_id, f]));
      const items = selection.map((s) => {
        const f = byRule[s.rule_id];
        const item = f.items.find((i) => i.path === s.path);
        const action = f.prefer_command
          ? { kind: "command", ...f.prefer_command }
          : f.permanent || (f.safety === "regenerable" && options.delete_regenerable)
            ? { kind: "delete" }
            : { kind: "trash" };
        const blockers = item.requires_closed.filter((b) => running.has(b)).map((b) => `Quit the app ${b} first`);
        if (f.permanent && !options.allow_permanent) {
          blockers.push("Deletes permanently: confirm emptying the Trash to include it");
        }
        return { rule_id: s.rule_id, rule_name: f.name, path: s.path, bytes: s.bytes, safety: f.safety, action, blockers };
      });
      const sum = (pred) => items.filter(pred).reduce((a, i) => a + i.bytes, 0);
      const ready = (i) => i.blockers.length === 0;
      const plan = {
        id: `demo-plan-${nextId++}`,
        items,
        trash_bytes: sum((i) => ready(i) && i.action.kind === "trash"),
        delete_bytes: sum((i) => ready(i) && i.action.kind === "delete"),
        command_bytes: sum((i) => ready(i) && i.action.kind === "command"),
        blocked_bytes: sum((i) => !ready(i)),
      };
      plans[plan.id] = plan;
      return plan;
    },
    execute_plan: ({ planId, dryRun }) => {
      const plan = plans[planId];
      const outcomes = plan.items.map((i) => ({
        path: i.path,
        rule_id: i.rule_id,
        action: i.action,
        bytes: i.bytes,
        status: i.blockers.length
          ? "skipped"
          : dryRun
            ? "would_clean"
            : { trash: "trashed", delete: "deleted", command: "command_ran" }[i.action.kind],
        message: i.blockers[0] ?? null,
        trashed_to: null,
      }));
      const total = (s) => outcomes.filter((o) => o.status === s).reduce((a, o) => a + o.bytes, 0);
      const before = available;
      if (!dryRun) {
        available += Math.round(total("deleted") * 0.97);
        const done = new Set(outcomes.filter((o) => o.status === "trashed" || o.status === "deleted").map((o) => o.path));
        findings = findings
          .map((f) => ({ ...f, items: f.items.filter((i) => !done.has(i.path)) }))
          .map((f) => ({ ...f, total_bytes: f.items.reduce((s, i) => s + i.bytes, 0) }))
          .filter((f) => f.items.length > 0);
      }
      return {
        run_id: "demo-run",
        plan_id: planId,
        dry_run: dryRun,
        outcomes,
        available_before: before,
        available_after: available,
        freed_bytes: available - before,
        trashed_bytes: total("trashed"),
        deleted_bytes: total("deleted"),
        skipped: outcomes.filter((o) => o.status === "skipped").length,
        failed: 0,
      };
    },
    undo_run: () => ({ restored: [], failed: [] }),
    list_snapshots: () => D.snapshots,
    reveal_path: () => null,
    "plugin:event|listen": ({ event, handler }) => {
      (listeners[event] ||= []).push(handler);
      return handler;
    },
    "plugin:event|unlisten": () => null,
  };

  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  window.__TAURI_INTERNALS__ = {
    transformCallback: (cb) => {
      const id = nextId++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback: (id) => callbacks.delete(id),
    invoke: async (cmd, args) => {
      const handler = handlers[cmd];
      if (!handler) throw `demo mode: ${cmd} is not available`;
      return handler(args || {});
    },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
  };
})();
