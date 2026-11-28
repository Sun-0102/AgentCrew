import { useEffect, useState } from "react";
import Dialog from "../../components/Dialog";
import Icon from "../../components/Icon";
import { archiveWikiNote, listWikiSources, type ArchiveNoteResult, type WikiSource } from "./api";

interface Props {
  initialTitle: string;
  initialContent: string;
  defaultSourceId?: string | null;
  sourceConversation?: string | null;
  onArchived: (result: ArchiveNoteResult) => void;
  onClose: () => void;
}

export default function WikiArchiveDialog({
  initialTitle,
  initialContent,
  defaultSourceId,
  sourceConversation,
  onArchived,
  onClose,
}: Props) {
  const [sources, setSources] = useState<WikiSource[]>([]);
  const [sourceId, setSourceId] = useState(defaultSourceId ?? "");
  const [title, setTitle] = useState(initialTitle);
  const [tagsText, setTagsText] = useState("业务沉淀");
  const [content, setContent] = useState(initialContent);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void listWikiSources()
      .then(list => {
        if (cancelled) return;
        setSources(list);
        if (!sourceId && list.length > 0) {
          setSourceId(defaultSourceId && list.some(s => s.id === defaultSourceId) ? defaultSourceId : list[0].id);
        }
      })
      .catch(cause => {
        if (!cancelled) setError(String(cause));
      });
    return () => { cancelled = true; };
  }, [defaultSourceId]);

  async function submit() {
    if (saving || !sourceId || !content.trim()) return;
    setSaving(true);
    setError(null);
    try {
      const tags = tagsText
        .split(/[,，\s]+/)
        .map(s => s.trim())
        .filter(Boolean);
      const res = await archiveWikiNote({
        sourceId,
        title: title.trim() || "AgentCrew 沉淀结论",
        content,
        tags,
        sourceConversation,
      });
      onArchived(res);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog title="沉淀到知识库（安全写入 00-inbox/）" onClose={() => { if (!saving) onClose(); }}>
      <form onSubmit={e => { e.preventDefault(); void submit(); }}>
        <label className="field-label" htmlFor="archive-source">目标知识库</label>
        <select
          id="archive-source"
          className="text-input"
          value={sourceId}
          onChange={e => setSourceId(e.target.value)}
          required
        >
          {!sources.length && <option value="">请先在「工作台设置 ➔ 知识库」接入目录</option>}
          {sources.map(s => (
            <option key={s.id} value={s.id}>
              {s.name}（写入 {s.inbox_subdir || "00-inbox"}/）
            </option>
          ))}
        </select>

        <label className="field-label" htmlFor="archive-title">笔记标题</label>
        <input
          id="archive-title"
          className="text-input"
          value={title}
          onChange={e => setTitle(e.target.value)}
          placeholder="例如：双11返点核算复盘"
          maxLength={80}
          required
        />

        <label className="field-label" htmlFor="archive-tags">标签（逗号分隔）</label>
        <input
          id="archive-tags"
          className="text-input"
          value={tagsText}
          onChange={e => setTagsText(e.target.value)}
          placeholder="例如：返点核算, Q4复盘"
        />

        <label className="field-label" htmlFor="archive-content">沉淀正文</label>
        <textarea
          id="archive-content"
          className="text-input"
          style={{ minHeight: 160, resize: "vertical", fontFamily: "inherit" }}
          value={content}
          onChange={e => setContent(e.target.value)}
          required
        />

        <p className="field-help">
          仅新增 Markdown 到收件箱目录，不会覆写现有正式页；写入后自动增量更新本地 FTS5 索引（若 LLM Wiki 在线则自动通知重扫）。
        </p>
        {error && <div className="inline-error" role="alert">{error}</div>}
        <div className="dialog-footer">
          <button type="button" className="outline-button" disabled={saving} onClick={onClose}>取消</button>
          <button type="submit" className="primary" disabled={saving || !sourceId || !content.trim()}>
            {saving ? "写入中…" : "确认沉淀"}
            <Icon name="check" size={15} />
          </button>
        </div>
      </form>
    </Dialog>
  );
}
