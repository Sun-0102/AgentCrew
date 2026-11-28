import { useEffect, useState } from "react";
import Icon from "../../components/Icon";
import {
  listWikiSources,
  searchWiki,
  setWikiBinding,
  WIKI_BINDING_NONE,
  type ExplicitWikiRef,
  type WikiSearchHit,
  type WikiSource,
} from "./api";

interface Props {
  conversationId: string;
  isGroup: boolean;
  isProject: boolean;
  explicitBindingSourceId: string | null;
  defaultPrivateSourceId: string | null;
  effectiveSourceId: string | null;
  selectedRefs: ExplicitWikiRef[];
  onBindingChanged: (explicitSourceId: string | null) => void;
  onToggleRef: (hit: WikiSearchHit) => void;
  onClose: () => void;
}

export default function WikiPickerPopover({
  conversationId,
  isGroup,
  isProject,
  explicitBindingSourceId,
  defaultPrivateSourceId,
  effectiveSourceId,
  selectedRefs,
  onBindingChanged,
  onToggleRef,
  onClose,
}: Props) {
  const [sources, setSources] = useState<WikiSource[]>([]);
  const [savingBinding, setSavingBinding] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<WikiSearchHit[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void listWikiSources()
      .then(list => {
        if (!cancelled) setSources(list);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    const timer = window.setTimeout(() => {
      void searchWiki({
        query,
        sourceId: effectiveSourceId || null,
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
  }, [query, effectiveSourceId]);

  const selectedSet = new Set(selectedRefs.map(r => `${r.source_id}:${r.rel_path}`));
  const defaultSourceName = sources.find(s => s.id === defaultPrivateSourceId)?.name;

  // Select value: "" = follow default (or none in project), "__none__" = explicitly disabled, "<id>" = bound source
  const selectValue = explicitBindingSourceId ?? "";

  async function handleChangeBinding(nextValue: string) {
    if (!conversationId || savingBinding) return;
    setSavingBinding(true);
    try {
      const targetSource = nextValue === "" ? null : nextValue;
      await setWikiBinding({
        conversationId,
        sourceId: targetSource,
      });
      onBindingChanged(targetSource);
    } finally {
      setSavingBinding(false);
    }
  }

  return (
    <div
      style={{
        position: "absolute",
        bottom: "calc(100% + 8px)",
        left: 0,
        width: 390,
        maxWidth: "min(390px, 86vw)",
        background: "var(--surface, #fff)",
        border: "1px solid var(--border)",
        borderRadius: 10,
        boxShadow: "0 12px 32px rgba(0,0,0,0.16)",
        padding: 10,
        zIndex: 40,
      }}
      role="dialog"
      aria-label="知识库与文档引用"
    >
      {/* 顶部 1 秒切换当前会话知识库（非群聊） */}
      {!isGroup && sources.length > 0 && conversationId && (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            paddingBottom: 8,
            marginBottom: 8,
            borderBottom: "1px solid var(--border)",
          }}
        >
          <span style={{ fontSize: 12, opacity: 0.75, flexShrink: 0 }}>本会话知识库</span>
          <select
            className="text-input"
            style={{ flex: 1, fontSize: 12, padding: "4px 8px" }}
            disabled={savingBinding}
            value={selectValue}
            onChange={e => void handleChangeBinding(e.target.value)}
          >
            {isProject ? (
              <option value="">不关联（默认，防代码污染）</option>
            ) : (
              <>
                <option value="">
                  {defaultSourceName ? `跟随默认（${defaultSourceName}）` : "跟随默认（不关联）"}
                </option>
                {defaultPrivateSourceId && (
                  <option value={WIKI_BINDING_NONE}>本会话不关联</option>
                )}
              </>
            )}
            {sources.map(s => (
              <option key={s.id} value={s.id}>
                {s.name}（{s.doc_count} 篇）
              </option>
            ))}
          </select>
          <button type="button" className="icon-button" onClick={onClose} aria-label="关闭">
            <Icon name="close" size={16} />
          </button>
        </div>
      )}

      <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
        <input
          className="text-input"
          style={{ flex: 1, fontSize: 13, padding: "6px 10px" }}
          value={query}
          onChange={e => setQuery(e.target.value)}
          placeholder="临时点名某篇文档（可选，仅本轮直传）…"
          autoFocus
        />
        {(isGroup || !sources.length || !conversationId) && (
          <button type="button" className="icon-button" onClick={onClose} aria-label="关闭">
            <Icon name="close" size={16} />
          </button>
        )}
      </div>

      <div style={{ maxHeight: 220, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }}>
        {loading ? (
          <p className="field-help" style={{ padding: "8px 4px" }}>检索中…</p>
        ) : !hits.length ? (
          <p className="field-help" style={{ padding: "8px 4px" }}>
            未找到匹配文档。可在「工作台设置 ➔ 知识库」添加本地文件夹或同步 LLM Wiki。
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
