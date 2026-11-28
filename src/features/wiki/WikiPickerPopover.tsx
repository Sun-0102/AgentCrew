import { useEffect, useId, useMemo, useRef, useState } from "react";
import Icon from "../../components/Icon";
import {
  searchWiki,
  WIKI_BINDING_NONE,
  type ExplicitWikiRef,
  type WikiBinding,
  type WikiSearchHit,
  type WikiSource,
} from "./api";

interface Props {
  conversationId: string;
  isGroup: boolean;
  isProject: boolean;
  disabled?: boolean;
  sources: WikiSource[];
  explicitBinding: WikiBinding | null;
  defaultPrivateSourceId: string | null;
  effectiveSourceIds: string[];
  selectedRefs: ExplicitWikiRef[];
  onOpen?: () => void;
  onSaveBinding: (nextSourceIds: string[] | null, explicitlyDisabled?: boolean) => Promise<void>;
  onToggleRef: (hit: WikiSearchHit) => void;
  onClearRefs: () => void;
}

function cleanSnippetText(raw: string): string {
  return raw
    .replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (_, target, alias) => alias || target)
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/__([^_]+)__/g, "$1")
    .replace(/(?:^|\s)#{1,6}\s+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export default function WikiPickerPopover({
  conversationId,
  isGroup,
  isProject,
  disabled = false,
  sources,
  explicitBinding,
  defaultPrivateSourceId,
  effectiveSourceIds,
  selectedRefs,
  onOpen,
  onSaveBinding,
  onToggleRef,
  onClearRefs,
}: Props) {
  const id = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [savingBinding, setSavingBinding] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<WikiSearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [position, setPosition] = useState({ left: 12, bottom: 72, width: 410 });

  const effectiveSet = useMemo(() => new Set(effectiveSourceIds), [effectiveSourceIds]);
  const selectedRefSet = useMemo(
    () => new Set(selectedRefs.map(r => `${r.source_id}:${r.rel_path}`)),
    [selectedRefs],
  );
  const effectiveKey = effectiveSourceIds.join(",");

  const isExplicitlyDisabled = explicitBinding?.source_id === WIKI_BINDING_NONE;
  const isFollowingDefault = !isGroup && !isProject && explicitBinding === null && !!defaultPrivateSourceId;
  const canRestoreDefault = !isGroup && !isProject && !!defaultPrivateSourceId && explicitBinding !== null;
  const showSourceSelector = !isGroup && !!conversationId;

  function updatePosition() {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const width = Math.min(410, window.innerWidth - 24);
    const left = Math.max(12, Math.min(rect.left, window.innerWidth - width - 12));
    const bottom = Math.max(12, window.innerHeight - rect.top + 10);
    setPosition({ left, bottom, width });
  }

  useEffect(() => {
    const close = () => panelRef.current?.hidePopover();
    window.addEventListener("resize", close);
    return () => window.removeEventListener("resize", close);
  }, []);

  useEffect(() => {
    if (!open) return;
    const timer = window.setTimeout(() => searchRef.current?.focus(), 40);
    return () => window.clearTimeout(timer);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    const timer = window.setTimeout(() => {
      void searchWiki({
        query,
        sourceIds: effectiveSourceIds.length > 0 ? effectiveSourceIds : null,
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
  }, [open, query, effectiveKey]);

  async function handleToggleSource(sourceId: string) {
    if (!conversationId || savingBinding) return;
    setSavingBinding(true);
    try {
      const next = effectiveSet.has(sourceId)
        ? effectiveSourceIds.filter(id => id !== sourceId)
        : [...effectiveSourceIds, sourceId];
      if (next.length === 0) {
        const needExplicitNone = !isProject && !!defaultPrivateSourceId;
        await onSaveBinding(null, needExplicitNone);
      } else {
        await onSaveBinding(next, false);
      }
    } finally {
      setSavingBinding(false);
    }
  }

  async function handleClearSources() {
    if (!conversationId || savingBinding) return;
    setSavingBinding(true);
    try {
      const needExplicitNone = !isProject && !!defaultPrivateSourceId;
      await onSaveBinding(null, needExplicitNone);
    } finally {
      setSavingBinding(false);
    }
  }

  async function handleRestoreDefault() {
    if (!conversationId || savingBinding) return;
    setSavingBinding(true);
    try {
      await onSaveBinding(null, false);
    } finally {
      setSavingBinding(false);
    }
  }

  const boundNames = useMemo(() => {
    const map = new Map(sources.map(s => [s.id, s.name]));
    return effectiveSourceIds.map(sid => map.get(sid) ?? sid);
  }, [sources, effectiveSourceIds]);

  const activeCount = effectiveSourceIds.length + selectedRefs.length;
  const triggerTitle = boundNames.length
    ? `已关联知识库：${boundNames.join("、")}${selectedRefs.length ? ` · 已点名 ${selectedRefs.length} 篇文档` : ""}`
    : selectedRefs.length
      ? `已点名 ${selectedRefs.length} 篇文档（点击管理知识库）`
      : "关联知识库或点名文档";

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={`icon-button wiki-picker-trigger ${open || activeCount > 0 ? "selected" : ""}`}
        title={triggerTitle}
        aria-label="知识库与文档引用"
        aria-expanded={open}
        aria-controls={id}
        aria-haspopup="dialog"
        popoverTarget={id}
        disabled={disabled}
        onClick={updatePosition}
      >
        <Icon name="file" size={18} />
        {activeCount > 0 && <span className="wiki-trigger-badge">{activeCount}</span>}
      </button>

      <div
        ref={panelRef}
        id={id}
        popover="auto"
        role="dialog"
        aria-label="知识库与文档引用"
        className="model-config-popover wiki-picker-popover"
        style={{ left: position.left, bottom: position.bottom, width: position.width }}
        onToggle={event => {
          const isOpen = event.newState === "open";
          setOpen(isOpen);
          if (isOpen) {
            updatePosition();
            onOpen?.();
          }
        }}
      >
        {showSourceSelector && (
          <div className="wiki-picker-section wiki-picker-sources-section">
            <div className="wiki-picker-section-header">
              <div className="wiki-picker-section-title">
                <span>本会话知识库</span>
                <span className={`wiki-picker-pill ${effectiveSourceIds.length > 0 ? "is-bound" : ""}`}>
                  {effectiveSourceIds.length > 0
                    ? isFollowingDefault
                      ? "默认"
                      : `已选 ${effectiveSourceIds.length}`
                    : isExplicitlyDisabled
                      ? "已取消"
                      : "未关联"}
                </span>
              </div>
              <div className="wiki-picker-section-actions">
                {canRestoreDefault && (
                  <button
                    type="button"
                    className="wiki-picker-text-btn"
                    disabled={savingBinding}
                    onClick={() => void handleRestoreDefault()}
                  >
                    恢复默认
                  </button>
                )}
                {effectiveSourceIds.length > 0 && (
                  <button
                    type="button"
                    className="wiki-picker-text-btn is-danger"
                    disabled={savingBinding}
                    onClick={() => void handleClearSources()}
                  >
                    取消关联
                  </button>
                )}
              </div>
            </div>

            {sources.length === 0 ? (
              <p className="wiki-picker-empty">
                尚未添加知识库，可在「工作台设置 ➔ 知识库」添加本地文件夹或同步 LLM Wiki。
              </p>
            ) : (
              <div className="wiki-source-list" role="listbox" aria-multiselectable="true" aria-label="选择知识库">
                {sources.map(source => {
                  const checked = effectiveSet.has(source.id);
                  const isDefault = !isProject && source.id === defaultPrivateSourceId;
                  return (
                    <button
                      key={source.id}
                      type="button"
                      role="option"
                      aria-selected={checked}
                      disabled={savingBinding}
                      className={`wiki-source-option ${checked ? "is-selected" : ""}`}
                      onClick={() => void handleToggleSource(source.id)}
                    >
                      <span className={`wiki-source-check ${checked ? "is-checked" : ""}`} aria-hidden="true">
                        {checked && <Icon name="check" size={12} />}
                      </span>
                      <span className="wiki-source-name">{source.name}</span>
                      {isDefault && <span className="wiki-source-tag">默认</span>}
                      <span className="wiki-source-count">{source.doc_count} 篇</span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        )}

        <div className="wiki-picker-section wiki-picker-docs-section">
          <div className="wiki-picker-section-header">
            <div className="wiki-picker-section-title">
              <span>临时点名文档</span>
              <span className="wiki-picker-hint">仅本轮直传</span>
            </div>
            {selectedRefs.length > 0 && (
              <div className="wiki-picker-section-actions">
                <span className="wiki-picker-pill is-bound">已选 {selectedRefs.length}</span>
                <button type="button" className="wiki-picker-text-btn" onClick={onClearRefs}>
                  清空
                </button>
              </div>
            )}
          </div>

          <div className="wiki-picker-search">
            <Icon name="search" size={14} />
            <input
              ref={searchRef}
              value={query}
              onChange={e => setQuery(e.target.value)}
              placeholder={
                effectiveSourceIds.length > 0
                  ? "在已关联知识库中搜索文档…"
                  : "在全部知识库中搜索文档…"
              }
            />
            {query && (
              <button
                type="button"
                className="wiki-picker-search-clear"
                onClick={() => {
                  setQuery("");
                  searchRef.current?.focus();
                }}
                aria-label="清空搜索"
              >
                <Icon name="close" size={13} />
              </button>
            )}
          </div>

          <div className="wiki-picker-hits">
            {loading ? (
              <p className="wiki-picker-empty">检索中…</p>
            ) : !hits.length ? (
              <p className="wiki-picker-empty">
                未找到匹配文档。可在「工作台设置 ➔ 知识库」添加本地文件夹或同步 LLM Wiki。
              </p>
            ) : (
              hits.map(hit => {
                const key = `${hit.source_id}:${hit.rel_path}`;
                const active = selectedRefSet.has(key);
                const snippet = hit.snippet ? cleanSnippetText(hit.snippet) : "";
                return (
                  <button
                    key={key}
                    type="button"
                    className={`wiki-hit-item ${active ? "is-active" : ""}`}
                    onClick={() => onToggleRef(hit)}
                  >
                    <div className="wiki-hit-top">
                      <strong className="wiki-hit-title">{hit.title}</strong>
                      <span className={`wiki-hit-badge ${active ? "is-active" : ""}`}>
                        {active ? (
                          <>
                            <Icon name="check" size={11} />
                            已选
                          </>
                        ) : (
                          effectiveSourceIds.length !== 1
                            ? hit.source_name
                            : hit.top_domain || hit.source_name
                        )}
                      </span>
                    </div>
                    <span className="wiki-hit-path">{hit.rel_path}</span>
                    {snippet && <span className="wiki-hit-snippet">{snippet}</span>}
                  </button>
                );
              })
            )}
          </div>
        </div>
      </div>
    </>
  );
}
