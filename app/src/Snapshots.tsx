import { useEffect, useState } from "react";
import { api, type SnapshotReport } from "./api";

const KIND: Record<string, string> = {
  time_machine: "Time Machine",
  os_update: "macOS update",
  other: "Other",
};

export function Snapshots() {
  const [report, setReport] = useState<SnapshotReport | null>(null);
  useEffect(() => {
    api.listSnapshots().then(setReport, (e) => setReport({ snapshots: [], explanation: "", thin_command: "", error: String(e) }));
  }, []);
  if (!report) return <p className="muted">Reading snapshots…</p>;
  return (
    <div className="snapshots">
      <p>{report.explanation}</p>
      {report.error && <p className="error">{report.error}</p>}
      {report.snapshots.length === 0 ? (
        <p className="empty">No APFS local snapshots on the startup disk.</p>
      ) : (
        <table className="plan">
          <thead>
            <tr>
              <th>Snapshot</th>
              <th>Kind</th>
            </tr>
          </thead>
          <tbody>
            {report.snapshots.map((s) => (
              <tr key={s.name}>
                <td className="path">{s.name}</td>
                <td>{KIND[s.kind]}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p className="small muted">
        OpenDisk does not delete snapshots. To remove Time Machine local snapshots yourself, run{" "}
        <code>{report.thin_command}</code> in Terminal. Their size cannot be measured without admin rights.
      </p>
    </div>
  );
}
