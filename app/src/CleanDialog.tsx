import { useState } from "react";
import { api, formatBytes, tildify, type Action, type Plan, type Report, type Status } from "./api";
import { SafetyBadge } from "./Findings";

function actionText(a: Action, trashName: string): string {
  switch (a.kind) {
    case "trash":
      return `Move to ${trashName}`;
    case "delete":
      return "Delete now";
    case "command":
      return `Run ${a.argv.join(" ")}`;
  }
}

function statusText(status: Status, trashName: string): string {
  switch (status) {
    case "would_clean":
      return "Would clean";
    case "trashed":
      return `Moved to ${trashName}`;
    case "deleted":
      return "Deleted";
    case "erased":
      return "Deleted permanently";
    case "command_ran":
      return "Command ran";
    case "covered_by_command":
      return "Covered by command";
    case "skipped":
      return "Skipped";
    case "failed":
      return "Failed";
  }
}

interface Props {
  home: string;
  pathSep: string;
  trashName: string;
  plan: Plan;
  onClose: () => void;
  onCleaned: (report: Report) => void;
}

/** Preview of a plan, then dry run or the real thing, then the results. */
export function CleanDialog({ home, pathSep, trashName, plan, onClose, onCleaned }: Props) {
  const [report, setReport] = useState<Report | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [undone, setUndone] = useState<string | null>(null);
  const ready = plan.items.filter((i) => i.blockers.length === 0);
  const blocked = plan.items.filter((i) => i.blockers.length > 0);

  const run = async (dryRun: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.executePlan(plan.id, dryRun);
      setReport(r);
      if (!dryRun) onCleaned(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const undo = async () => {
    if (!report) return;
    const res = await api.undoRun(report.run_id);
    setUndone(
      `Put back ${res.restored.length} item(s)` +
        (res.failed.length ? `; ${res.failed.length} could not be restored (${res.failed[0][1]})` : ""),
    );
  };

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true">
      <div className="modal">
        {!report || report.dry_run ? (
          <>
            <h2>{report?.dry_run ? "Dry run result" : "Preview"}</h2>
            <div className="totals">
              <div>
                <span className="muted">To {trashName}</span>
                <strong>{formatBytes(plan.trash_bytes)}</strong>
              </div>
              <div>
                <span className="muted">Delete now</span>
                <strong>{formatBytes(plan.delete_bytes)}</strong>
              </div>
              <div>
                <span className="muted">Via official tool</span>
                <strong>{formatBytes(plan.command_bytes)}</strong>
              </div>
              <div>
                <span className="muted">Blocked</span>
                <strong>{formatBytes(plan.blocked_bytes)}</strong>
              </div>
            </div>
            {report?.dry_run && (
              <p className="note">
                Every check ran and nothing was changed: {report.outcomes.filter((o) => o.status === "would_clean").length}{" "}
                item(s) would be cleaned, {report.skipped} skipped.
              </p>
            )}
            <table className="plan">
              <thead>
                <tr>
                  <th>Item</th>
                  <th>Action</th>
                  <th className="num">Size</th>
                </tr>
              </thead>
              <tbody>
                {[...ready, ...blocked].map((i) => {
                  const outcome = report?.outcomes.find((o) => o.path === i.path);
                  return (
                    <tr key={i.path} className={i.blockers.length ? "blocked" : ""}>
                      <td>
                        <div className="path" title={i.path}>
                          {tildify(i.path, home, pathSep)}
                        </div>
                        <div className="small muted">
                          {i.rule_name} <SafetyBadge safety={i.safety} />
                        </div>
                        {i.blockers.map((b) => (
                          <div key={b} className="small warn">
                            {b}
                          </div>
                        ))}
                        {outcome?.message && i.blockers.length === 0 && (
                          <div className="small warn">{outcome.message}</div>
                        )}
                      </td>
                      <td>
                        {i.blockers.length ? "Skip" : actionText(i.action, trashName)}
                        {outcome && <div className="small muted">{statusText(outcome.status, trashName)}</div>}
                      </td>
                      <td className="num">{formatBytes(i.bytes)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <p className="small muted">
              Each item is checked again right before it is cleaned: it must still be covered by its rule, not be a
              symlink, be unchanged since this preview, and its app must be closed.
            </p>
            {error && <p className="error">{error}</p>}
            <div className="actions">
              <button className="ghost" onClick={onClose} disabled={busy}>
                Cancel
              </button>
              <button onClick={() => run(true)} disabled={busy || ready.length === 0}>
                Dry run
              </button>
              <button className="primary" onClick={() => run(false)} disabled={busy || ready.length === 0}>
                {busy ? "Working…" : `Clean ${ready.length} item(s)`}
              </button>
            </div>
          </>
        ) : (
          <>
            <h2>Done</h2>
            <div className="freed">
              <span className="muted">Space freed on disk (measured)</span>
              <strong>{formatBytes(Math.max(0, report.freed_bytes))}</strong>
              <span className="small muted">
                {formatBytes(report.available_before)} → {formatBytes(report.available_after)} available
              </span>
            </div>
            {report.trashed_bytes > 0 && (
              <p className="note">
                {formatBytes(report.trashed_bytes)} went to the {trashName}. That space is freed when you empty it; until
                then you can put it back.
              </p>
            )}
            {report.freed_bytes < report.deleted_bytes * 0.8 && report.deleted_bytes > 0 && (
              <p className="note">
                Less space came back than the deleted size. Usually clones or hardlinks share those blocks, or a local
                snapshot still references them (see Snapshots).
              </p>
            )}
            <ul className="outcomes">
              {report.outcomes.map((o) => (
                <li key={o.path} className={o.status}>
                  <span className="path" title={o.path}>
                    {tildify(o.path, home, pathSep)}
                  </span>
                  <span>{statusText(o.status, trashName)}</span>
                  {o.message && <span className="small warn">{o.message}</span>}
                </li>
              ))}
            </ul>
            {undone && <p className="note">{undone}</p>}
            <div className="actions">
              {report.trashed_bytes > 0 && !undone && (
                <button className="ghost" onClick={undo}>
                  Undo (put back from {trashName})
                </button>
              )}
              <button className="primary" onClick={onClose}>
                Close
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
