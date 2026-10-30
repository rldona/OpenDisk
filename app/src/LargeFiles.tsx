import { useEffect, useMemo, useState } from "react";
import { api, formatBytes, tildify, type LargeFile, type Selection } from "./api";

const THRESHOLDS: [number, string][] = [
  [100, "100 MB"],
  [500, "500 MB"],
  [1024, "1 GB"],
  [5 * 1024, "5 GB"],
];

interface Props {
  home: string;
  pathSep: string;
  fileManager: string;
  trashName: string;
  selected: Map<string, Selection>;
  onChange: (next: Map<string, Selection>) => void;
  /** Changes when a new scan finishes, so the list is fetched again. */
  scanKey: string;
  /** Changes after cleaning, so the list drops what was removed. */
  refreshKey: number;
}

/** Every big file under the scanned folder, whether a rule covers it or not. */
export function LargeFiles({ home, pathSep, fileManager, trashName, selected, onChange, scanKey, refreshKey }: Props) {
  const [minMb, setMinMb] = useState(500);
  const [rows, setRows] = useState<LargeFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    api
      .getLargeFiles(minMb * 1024 * 1024)
      .then((files) => {
        if (!alive) return;
        setRows(files);
        setError(null);
      })
      .catch((e) => alive && setError(String(e)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [minMb, scanKey, refreshKey]);

  const totals = useMemo(() => {
    const total = rows.reduce((s, f) => s + f.size, 0);
    const cleanable = rows.filter((f) => f.cleanable);
    return {
      total,
      cleanable: cleanable.length,
      cleanableBytes: cleanable.reduce((s, f) => s + f.size, 0),
    };
  }, [rows]);

  const changeThreshold = (next: number) => {
    // Selections picked here have no rule; drop them so the footer never
    // counts something that is no longer visible. Rule findings stay.
    onChange(new Map([...selected].filter(([, s]) => s.rule_id !== null)));
    setMinMb(next);
  };

  const toggle = (f: LargeFile) => {
    if (!f.cleanable) return;
    const next = new Map(selected);
    if (next.has(f.path)) {
      next.delete(f.path);
    } else {
      next.set(f.path, { rule_id: null, path: f.path, bytes: f.size });
    }
    onChange(next);
  };

  return (
    <div className="large-files">
      <div className="large-controls">
        <label>
          Files larger than{" "}
          <select value={minMb} onChange={(e) => changeThreshold(Number(e.target.value))}>
            {THRESHOLDS.map(([mb, label]) => (
              <option key={mb} value={mb}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <span className="muted">
          {rows.length} file{rows.length === 1 ? "" : "s"} · {formatBytes(totals.total)} · {totals.cleanable} cleanable
          {totals.cleanable > 0 && ` (${formatBytes(totals.cleanableBytes)})`}
        </span>
      </div>

      <p className="small muted">
        Files outside your home folder can be seen and revealed here, but OpenDisk only removes what is inside your home
        folder. Everything selected goes to the {trashName} after the preview. To see files outside your home folder,
        scan the whole disk.
      </p>

      {loading && <p className="muted">Reading files…</p>}
      {error && <p className="error">{error}</p>}
      {!loading && !error && rows.length === 0 && (
        <p className="empty">No files larger than {THRESHOLDS.find(([mb]) => mb === minMb)?.[1] ?? `${minMb} MB`}.</p>
      )}

      <ul className="large-list">
        {rows.map((f) => {
          const on = selected.has(f.path);
          return (
            <li key={f.path} className={`${f.cleanable ? "" : "protected"}${on ? " selected" : ""}`}>
              <input
                type="checkbox"
                checked={on}
                disabled={!f.cleanable}
                onChange={() => toggle(f)}
                aria-label={`Select ${f.path}`}
              />
              <div className="large-main">
                <div className="path" title={f.path}>
                  {tildify(f.path, home, pathSep)}
                </div>
                <div className="small muted">
                  {f.modified ? new Date(f.modified * 1000).toLocaleDateString() : "unknown date"}
                  {f.protected && ` · protected: ${f.protected}`}
                </div>
              </div>
              <span className="large-size">{formatBytes(f.size)}</span>
              <button className="ghost small" onClick={() => api.revealPath(f.path)}>
                Show in {fileManager}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
