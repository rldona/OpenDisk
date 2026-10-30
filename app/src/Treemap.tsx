import { useEffect, useMemo, useRef, useState } from "react";
import { hierarchy, treemap, treemapSquarify, type HierarchyRectangularNode } from "d3-hierarchy";
import { api, formatBytes, tildify, type TreeNode } from "./api";

const PALETTE = ["#3b82f6", "#14b8a6", "#f59e0b", "#8b5cf6", "#ef4444", "#10b981", "#ec4899", "#6366f1", "#84cc16", "#f97316"];

function useSize<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => {
    if (!ref.current) return;
    const ro = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setSize({ width: Math.floor(width), height: Math.floor(height) });
    });
    ro.observe(ref.current);
    return () => ro.disconnect();
  }, []);
  return [ref, size] as const;
}

/** Disk map of the last scan, hidden folders included, two levels at a time. */
export function Treemap({ home, fileManager, pathSep, root }: { home: string; fileManager: string; pathSep: string; root: string }) {
  const [path, setPath] = useState(root);
  const [tree, setTree] = useState<TreeNode | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ref, size] = useSize<HTMLDivElement>();

  useEffect(() => setPath(root), [root]);
  useEffect(() => {
    let alive = true;
    api
      .getTree(path, 2)
      .then((t) => alive && (setTree(t), setError(null)))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [path]);

  const layout = useMemo(() => {
    if (!tree || size.width === 0) return [];
    const h = hierarchy<TreeNode>(tree, (d) => d.children)
      .sum((d) => (d.children.length ? 0 : d.size))
      .sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
    const t = treemap<TreeNode>()
      .tile(treemapSquarify)
      .size([size.width, size.height])
      .paddingInner(2)
      .paddingTop((n) => (n.depth === 1 ? 20 : 0))
      .round(true);
    return t(h).descendants().filter((n) => n.depth > 0) as HierarchyRectangularNode<TreeNode>[];
  }, [tree, size]);

  const crumbs = useMemo(() => {
    const parts: { label: string; path: string }[] = [{ label: tildify(root, home, pathSep), path: root }];
    if (path !== root && path.startsWith(root)) {
      let acc = root;
      for (const seg of path.slice(root.length).split(/[\\/]/).filter(Boolean)) {
        acc = `${acc}${acc.endsWith(pathSep) ? "" : pathSep}${seg}`;
        parts.push({ label: seg, path: acc });
      }
    }
    return parts;
  }, [path, root, home, pathSep]);

  const topIndex = new Map<TreeNode, number>();
  tree?.children.forEach((c, i) => topIndex.set(c, i));

  return (
    <div className="treemap-wrap">
      <div className="crumbs">
        {crumbs.map((c, i) => (
          <span key={c.path}>
            {i > 0 && <span className="sep">{pathSep}</span>}
            <button className="link" onClick={() => setPath(c.path)} disabled={c.path === path}>
              {c.label}
            </button>
          </span>
        ))}
        {tree && <span className="muted"> — {formatBytes(tree.size)}</span>}
        {path !== root && (
          <button className="ghost small" onClick={() => setPath(crumbs[crumbs.length - 2].path)}>
            ↑ Up
          </button>
        )}
      </div>
      {error && <p className="error">{error}</p>}
      <div className="treemap" ref={ref}>
        {layout.map((n) => {
          const top = n.depth === 1 ? n : n.parent!;
          const color = PALETTE[(topIndex.get(top.data) ?? 0) % PALETTE.length];
          const w = n.x1 - n.x0;
          const h = n.y1 - n.y0;
          const hidden = n.data.name.startsWith(".");
          const canOpen = !n.data.other && (n.data.children.length > 0 || n.depth === 2) && !n.data.denied;
          return (
            <div
              key={`${n.depth}:${n.data.path}:${n.data.name}`}
              className={`tile depth${n.depth}${n.data.other ? " other" : ""}${hidden ? " hidden-dir" : ""}`}
              style={{
                left: n.x0,
                top: n.y0,
                width: w,
                height: h,
                background: n.depth === 1 ? `${color}33` : `${color}${n.data.other ? "55" : "aa"}`,
                borderColor: color,
              }}
              title={`${tildify(n.data.path, home, pathSep)}${n.data.other ? ` (${n.data.name})` : ""}\n${formatBytes(n.data.size)}${n.data.denied ? "\nNo access" : ""}`}
              onClick={(e) => {
                e.stopPropagation();
                if (canOpen) setPath(n.data.path);
              }}
            >
              {w > 50 && h > 16 && (
                <span className="tile-label">
                  {n.data.name}
                  {hidden && n.depth === 1 && <em className="tag">hidden</em>}
                  {w > 110 && <span className="tile-size"> {formatBytes(n.data.size)}</span>}
                </span>
              )}
            </div>
          );
        })}
      </div>
      <p className="hint">Click a block to open it. Folders starting with a dot are hidden in {fileManager} but count here.</p>
    </div>
  );
}
