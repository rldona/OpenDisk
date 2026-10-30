import { useMemo, useState } from "react";
import { api, formatBytes, tildify, type Finding, type Safety, type Selection } from "./api";

export const SAFETY_TEXT: Record<Safety, { label: string; hint: string }> = {
  regenerable: { label: "Regenerable", hint: "A cache the tool rebuilds on its own." },
  review: { label: "Review", hint: "Rebuildable but costly to get back. Goes to the Trash or Recycle Bin." },
  personal: { label: "Personal", hint: "Your own data. Only ever moved to the Trash or Recycle Bin." },
};

/** Renders `backticked` spans of rule text as code. */
export function RuleText({ text }: { text: string }) {
  return (
    <>
      {text.split("`").map((part, i) => (i % 2 === 1 ? <code key={i}>{part}</code> : part))}
    </>
  );
}

export function SafetyBadge({ safety }: { safety: Safety }) {
  return (
    <span className={`badge ${safety}`} title={SAFETY_TEXT[safety].hint}>
      {SAFETY_TEXT[safety].label}
    </span>
  );
}

interface Props {
  home: string;
  /** "Finder" on macOS, "Files" on Linux, "File Explorer" on Windows. */
  fileManager: string;
  pathSep: string;
  findings: Finding[];
  selected: Map<string, Selection>;
  onChange: (next: Map<string, Selection>) => void;
}

export function Findings({ home, fileManager, pathSep, findings, selected, onChange }: Props) {
  const [open, setOpen] = useState<Set<string>>(new Set());

  const groups = useMemo(() => {
    const byCat = new Map<string, { label: string; total: number; findings: Finding[] }>();
    for (const f of findings) {
      const g = byCat.get(f.category) ?? { label: f.category_label, total: 0, findings: [] };
      g.total += f.total_bytes;
      g.findings.push(f);
      byCat.set(f.category, g);
    }
    return [...byCat.values()].sort((a, b) => b.total - a.total);
  }, [findings]);

  const toggleItems = (f: Finding, paths: string[], on: boolean) => {
    const next = new Map(selected);
    for (const p of paths) {
      const item = f.items.find((i) => i.path === p)!;
      if (on) next.set(p, { rule_id: f.rule_id, path: p, bytes: item.bytes });
      else next.delete(p);
    }
    onChange(next);
  };

  if (findings.length === 0) {
    return <p className="empty">No findings. Either the disk is tidy or the scan has not run yet.</p>;
  }

  return (
    <div className="findings">
      {groups.map((g) => (
        <section key={g.label} className="category">
          <h3>
            {g.label} <span className="muted">{formatBytes(g.total)}</span>
          </h3>
          {g.findings.map((f) => {
            const paths = f.items.map((i) => i.path);
            const count = paths.filter((p) => selected.has(p)).length;
            const isOpen = open.has(f.rule_id);
            return (
              <div key={f.rule_id} className={`finding${count > 0 ? " has-selection" : ""}`}>
                <div className="finding-head">
                  <input
                    type="checkbox"
                    aria-label={`Select ${f.name}`}
                    checked={count === paths.length}
                    ref={(el) => {
                      if (el) el.indeterminate = count > 0 && count < paths.length;
                    }}
                    onChange={(e) => toggleItems(f, paths, e.target.checked)}
                  />
                  <button
                    className="finding-title"
                    onClick={() => {
                      const next = new Set(open);
                      if (isOpen) next.delete(f.rule_id);
                      else next.add(f.rule_id);
                      setOpen(next);
                    }}
                  >
                    <span className="chev">{isOpen ? "▾" : "▸"}</span>
                    <strong>{f.name}</strong>
                    <SafetyBadge safety={f.safety} />
                    {f.permanent && <span className="badge permanent">Permanent</span>}
                    <span className="muted">
                      {f.items.length} {f.items.length === 1 ? "item" : "items"}
                    </span>
                  </button>
                  <span className="size">{formatBytes(f.total_bytes)}</span>
                </div>
                <p className="explanation">
                  <RuleText text={f.explanation} />
                </p>
                {f.prefer_command && (
                  <p className="command-note">
                    Cleans with <code>{f.prefer_command.argv.join(" ")}</code>: {f.prefer_command.description}
                  </p>
                )}
                {isOpen && (
                  <ul className="items">
                    {f.items.map((i) => (
                      <li key={i.path}>
                        <label>
                          <input
                            type="checkbox"
                            checked={selected.has(i.path)}
                            onChange={(e) => toggleItems(f, [i.path], e.target.checked)}
                          />
                          <span className="path" title={i.path}>
                            {tildify(i.path, home, pathSep)}
                          </span>
                        </label>
                        {i.requires_closed.length > 0 && (
                          <span className="muted small" title="Must be closed before cleaning">
                            needs {i.requires_closed.join(", ")} closed
                          </span>
                        )}
                        <button className="ghost small" onClick={() => api.revealPath(i.path)}>
                          Show in {fileManager}
                        </button>
                        <span className="size">{formatBytes(i.bytes)}</span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            );
          })}
        </section>
      ))}
    </div>
  );
}
