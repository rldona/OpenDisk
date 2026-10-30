import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  formatBytes,
  type AppInfo,
  type DiskInfo,
  type FdaStatus,
  type Finding,
  type Plan,
  type ScanDone,
  type ScanProgress,
  type Selection,
} from "./api";
import { Findings } from "./Findings";
import { Treemap } from "./Treemap";
import { Snapshots } from "./Snapshots";
import { CleanDialog } from "./CleanDialog";

type Tab = "findings" | "map" | "snapshots";
type Phase = "idle" | "scanning" | "ready";

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [disk, setDisk] = useState<DiskInfo | null>(null);
  const [fda, setFda] = useState<FdaStatus | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [target, setTarget] = useState<"home" | "volume">("home");
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [done, setDone] = useState<ScanDone | null>(null);
  const [findings, setFindings] = useState<Finding[]>([]);
  const [selected, setSelected] = useState<Map<string, Selection>>(new Map());
  const [tab, setTab] = useState<Tab>("findings");
  const [deleteRegenerable, setDeleteRegenerable] = useState(false);
  const [allowPermanent, setAllowPermanent] = useState(false);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refreshDisk = useCallback(() => api.diskInfo().then(setDisk).catch(() => {}), []);
  const refreshFda = useCallback(() => api.fdaStatus().then(setFda).catch(() => {}), []);

  useEffect(() => {
    api.appInfo().then(setInfo);
    refreshDisk();
    refreshFda();
    const subs = [
      api.onScanProgress(setProgress),
      api.onScanDone(async (d) => {
        setDone(d);
        if (d.error) {
          setError(d.error);
          setPhase("idle");
          return;
        }
        setFindings(await api.getFindings());
        setSelected(new Map());
        setPhase("ready");
      }),
    ];
    // Re-check Full Disk Access when the user comes back from System Settings.
    const onFocus = () => refreshFda();
    window.addEventListener("focus", onFocus);
    return () => {
      subs.forEach((p) => p.then((un) => un()));
      window.removeEventListener("focus", onFocus);
    };
  }, [refreshDisk, refreshFda]);

  const startScan = async () => {
    setError(null);
    setProgress(null);
    setPhase("scanning");
    try {
      await api.startScan(target);
    } catch (e) {
      setError(String(e));
      setPhase(done ? "ready" : "idle");
    }
  };

  const selectedBytes = useMemo(() => [...selected.values()].reduce((s, i) => s + i.bytes, 0), [selected]);
  const selectsPermanent = useMemo(
    () => findings.some((f) => f.permanent && f.items.some((i) => selected.has(i.path))),
    [findings, selected],
  );

  const preview = async () => {
    setError(null);
    try {
      setPlan(
        await api.createPlan([...selected.values()], {
          delete_regenerable: deleteRegenerable,
          allow_permanent: allowPermanent,
        }),
      );
    } catch (e) {
      setError(String(e));
    }
  };

  const home = info?.home ?? "";
  const isMac = (info?.os ?? "macos") === "macos";
  const fileManager = info?.file_manager ?? (isMac ? "Finder" : "Files");
  const trashName = info?.trash_name ?? "Trash";
  const pathSep = info?.path_sep ?? "/";
  const volumeLabel = isMac ? "Whole data volume" : `Whole disk (${info?.data_volume ?? "/"})`;
  const used = disk ? disk.total - disk.available : 0;

  return (
    <div className="app">
      <header>
        <div className="brand">
          <span className="logo" aria-hidden>
            ◔
          </span>
          OpenDisk
        </div>
        {disk && (
          <div className="diskbar" title={`${formatBytes(used)} used of ${formatBytes(disk.total)}`}>
            <div className="bar">
              <div style={{ width: `${(used / disk.total) * 100}%` }} />
            </div>
            <span>
              <strong>{formatBytes(disk.available)}</strong> free of {formatBytes(disk.total)}
            </span>
          </div>
        )}
        <div className="scan-controls">
          <select value={target} onChange={(e) => setTarget(e.target.value as "home" | "volume")} disabled={phase === "scanning"}>
            <option value="home">Home folder</option>
            <option value="volume">{volumeLabel}</option>
          </select>
          {phase === "scanning" ? (
            <button onClick={() => api.cancelScan()}>Cancel</button>
          ) : (
            <button className="primary" onClick={startScan}>
              {done ? "Scan again" : "Scan"}
            </button>
          )}
        </div>
      </header>

      {fda?.state === "denied" && (
        <div className="banner">
          <div>
            <strong>Give OpenDisk Full Disk Access to see everything.</strong>
            <p>
              Without it macOS hides Mail, Messages, the Trash, Safari and some app data, so the scan under-counts.
              OpenDisk never sends anything anywhere. In System Settings › Privacy & Security › Full Disk Access, turn on
              OpenDisk (add it with “+” if it's not listed), then come back.
            </p>
          </div>
          <div className="banner-actions">
            <button className="primary" onClick={() => api.openFdaSettings()}>
              Open System Settings
            </button>
            <button className="ghost" onClick={refreshFda}>
              Check again
            </button>
          </div>
        </div>
      )}

      {error && <div className="error-bar">{error}</div>}

      <main>
        {phase === "idle" && (
          <div className="welcome">
            <h1>See what fills your disk</h1>
            <p>
              OpenDisk scans {target === "home" ? "your home folder" : "the whole data volume"}, hidden folders included,
              and explains every cache it finds: what it is, whether it comes back on its own, and what must be closed
              first. Nothing is touched until you preview and confirm, and most things go to the {trashName}.
            </p>
            <button className="primary big" onClick={startScan}>
              Scan {target === "home" ? "home folder" : "data volume"}
            </button>
          </div>
        )}

        {phase === "scanning" && (
          <div className="progress">
            <div className="spinner" aria-hidden />
            <h2>{progress?.phase === "findings" ? "Matching rules…" : "Scanning…"}</h2>
            <div className="counters">
              <div>
                <strong>{(progress?.files ?? 0).toLocaleString()}</strong>
                <span className="muted">files</span>
              </div>
              <div>
                <strong>{(progress?.dirs ?? 0).toLocaleString()}</strong>
                <span className="muted">folders</span>
              </div>
              <div>
                <strong>{formatBytes(progress?.bytes ?? 0)}</strong>
                <span className="muted">measured</span>
              </div>
              <div>
                <strong>{((progress?.elapsed_ms ?? 0) / 1000).toFixed(1)}s</strong>
                <span className="muted">elapsed</span>
              </div>
            </div>
            {(progress?.errors ?? 0) > 0 && (
              <p className="small muted">{progress!.errors.toLocaleString()} items could not be read</p>
            )}
          </div>
        )}

        {phase === "ready" && done && (
          <>
            <div className="summary">
              <div>
                <span className="muted">Scanned</span>
                <strong>{formatBytes(done.total_bytes)}</strong>
                <span className="small muted">
                  {done.files.toLocaleString()} files in {(done.duration_ms / 1000).toFixed(1)}s
                  {done.cancelled && " (cancelled — partial)"}
                </span>
              </div>
              <div>
                <span className="muted">Recoverable (all findings)</span>
                <strong>{formatBytes(findings.reduce((s, f) => s + f.total_bytes, 0))}</strong>
                <span className="small muted">{findings.length} rules matched</span>
              </div>
              {done.denied.length > 0 && (
                <div>
                  <span className="muted">Not readable</span>
                  <strong>{done.denied.length}+ folders</strong>
                  <span className="small muted" title={done.denied.join("\n")}>
                    hover to see which
                  </span>
                </div>
              )}
            </div>
            <nav className="tabs">
              {(
                [
                  ["findings", "Findings"],
                  ["map", "Disk map"],
                  ...(isMac ? ([["snapshots", "Snapshots"]] as const) : []),
                ] as const
              ).map(([id, label]) => (
                <button key={id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>
                  {label}
                </button>
              ))}
            </nav>
            {tab === "findings" && (
              <Findings
                home={home}
                fileManager={fileManager}
                pathSep={pathSep}
                findings={findings}
                selected={selected}
                onChange={setSelected}
              />
            )}
            {tab === "map" && <Treemap home={home} fileManager={fileManager} pathSep={pathSep} root={done.root} />}
            {tab === "snapshots" && <Snapshots />}
          </>
        )}
      </main>

      {phase === "ready" && tab === "findings" && selected.size > 0 && (
        <footer className="selection-bar">
          <span>
            <strong>{selected.size}</strong> selected · <strong>{formatBytes(selectedBytes)}</strong>
          </span>
          <label title={`Regenerable caches are deleted right away instead of going to the ${trashName}, so the space comes back now.`}>
            <input type="checkbox" checked={deleteRegenerable} onChange={(e) => setDeleteRegenerable(e.target.checked)} />
            Delete regenerable caches immediately
          </label>
          {selectsPermanent && (
            <label className="warn">
              <input type="checkbox" checked={allowPermanent} onChange={(e) => setAllowPermanent(e.target.checked)} />
              Empty the {trashName} items for good
            </label>
          )}
          <button className="ghost" onClick={() => setSelected(new Map())}>
            Deselect all
          </button>
          <button className="primary" onClick={preview}>
            Preview…
          </button>
        </footer>
      )}

      {plan && (
        <CleanDialog
          home={home}
          pathSep={pathSep}
          trashName={trashName}
          plan={plan}
          onClose={() => setPlan(null)}
          onCleaned={async () => {
            setFindings(await api.getFindings());
            setSelected(new Map());
            refreshDisk();
          }}
        />
      )}
    </div>
  );
}
