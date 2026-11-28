import { useEffect, useState } from "react";
import Icon from "../../components/Icon";
import { searchWiki, type ExplicitWikiRef, type WikiSearchHit } from "./api";

interface Props {
  boundSourceId?: string | null;
  selectedRefs: ExplicitWikiRef[];
  onToggleRef: (hit: WikiSearchHit) => void;
  onClose: () => void;
}

export default function WikiPickerPopover({ boundSourceId, selectedRefs, onToggleRef, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<WikiSearchHit[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    const timer = window.setTimeout(() => {
      void searchWiki({
        query,
        sourceId: boundSourceId || null,
        limit: 20,
        offset: 0,
      })
        .then(items => {
          if (!cancelled) setHits(items);
        })
        .catch(() => {
          if (!cancelled) setHits([]);
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    }, 120);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query, boundSourceId]);

  const selectedSet = new Set(selectedRefs.map(r => `${r.source_id}:${r.rel_path}`));

  return (
    <div
      style={{
        position: "absolute",
        bottom: "calc(100% + 8px)",
        left: 0,
        width: 380,
        maxWidth: "min(380px, 85vw)",
        background: "var(--surface, #fff)",
        border: "1px solid var(--border)",
        borderRadius: 10,
        boxShadow: "0 12px 32px rgba(0,0,0,0.16)",
        padding: 10,
        zIndex: 40,
      }}
      role="dialog"
      aria-label="点名引用知识库文档"
    >
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
        <input
          className="text-input"
          style={{ flex: 1, fontSize: 13, padding: "6px 10px" }}
          value={query}
          onChange={e => setQuery(e.target.value)}
          placeholder="搜索业务知识库文档（仅本轮精准直传）…"
          autoFocus
        />
        <button type="button" className="icon-button" onClick={onClose} aria-label="关闭">
          <Icon name="close" size={16} />
        </button>
      </div>
      <div style={{ maxHeight: 240, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }}>
        {loading ? (
          <p className="field-help" style={{ padding: "8px 4px" }}>检索中…</p>
        ) : !hits.length ? (
          <p className="field-help" style={{ padding: "8px 4px" }}>
            未找到匹配文档。可在「工作台设置 ➔ 知识库」接入 LLM Wiki 或本地目录。
          </p>
        ) : (
          hits.map(hit => {
            const key = `${hit.source_id}:${hit.rel_path}`;
            const active = selectedSet.has(key);
            return (
              <button
                key={key}
                type="button"
                onClick={() => onToggleRef(hit)}
                style={{
                  textAlign: "left",
                  padding: "7px 9px",
                  borderRadius: 7,
                  border: active ? "1px solid var(--accent)" : "1px solid var(--border)",
                  background: active ? "var(--accent-soft, rgba(59,130,246,0.1))" : "transparent",
                  cursor: "pointer",
                  display: "flex",
                  flexDirection: "column",
                  gap: 2,
                }}
              >
                <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 6 }}>
                  <strong style={{ fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {hit.title}
                  </strong>
                  <span style={{ fontSize: 11, opacity: 0.65, flexShrink: 0 }}>
                    {active ? "已选 ✓" : hit.top_domain || hit.source_name}
                  </span>
                </div>
                <span style={{ fontSize: 11, opacity: 0.65, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {hit.rel_path}
                </span>
                {hit.snippet && (
                  <span style={{ fontSize: 11.5, opacity: 0.8, display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}>
                    {hit.snippet}
                  </span>
                )}
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}
