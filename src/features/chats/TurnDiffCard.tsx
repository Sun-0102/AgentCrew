import { useState } from "react";
import Icon from "../../components/Icon";
import { desktop } from "./api";
import { gitTurnRevert, type TurnDiffSummary } from "../git/api";
import "./turn-diff.css";

interface Props {
  summary: TurnDiffSummary;
  active?: boolean;
  activeFile?: string | null;
  onViewDiff: (filePath?: string) => void;
  onNotify: (text: string) => void;
  onReverted?: () => void;
}

function splitPath(fullPath: string): { dir: string; name: string } {
  const normalized = fullPath.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  if (idx === -1) return { dir: "", name: normalized };
  return { dir: normalized.slice(0, idx + 1), name: normalized.slice(idx + 1) };
}

export default function TurnDiffCard({
  summary,
  active = false,
  activeFile = null,
  onViewDiff,
  onNotify,
  onReverted,
}: Props) {
  const [confirming, setConfirming] = useState(false);
  const [reverting, setReverting] = useState(false);
  const [reverted, setReverted] = useState(false);

  const count = summary.files.length;
  if (count === 0) return null;

  const isSingle = count === 1;
  const singleFile = isSingle ? summary.files[0] : null;
  const singleParts = singleFile ? splitPath(singleFile.path) : null;
  const canRevert = desktop && Boolean(summary.from_git && summary.repo_root && summary.before_tree);

  async function handleRevert() {
    if (!canRevert || reverting) return;
    setReverting(true);
    try {
      const res = await gitTurnRevert(
        summary.repo_root,
        summary.before_tree,
        summary.files.map(f => f.path),
      );
      setReverted(true);
      setConfirming(false);
      onNotify(res.message);
      onReverted?.();
    } catch (cause) {
      onNotify(`撤销失败：${String(cause)}`);
    } finally {
      setReverting(false);
    }
  }

  return (
    <div className={`turn-diff-card ${active ? "is-active" : ""}`}>
      <div className="turn-diff-header">
        <button
          type="button"
          className="turn-diff-lead"
          onClick={() => onViewDiff(summary.files[0]?.path)}
          title={singleFile ? singleFile.path : `查看本轮 ${count} 个文件的变更`}
        >
          <span className="turn-diff-icon" aria-hidden="true">
            <Icon name="diff" size={16} />
          </span>
          <span className="turn-diff-title-group">
            <span className="turn-diff-title">
              {isSingle && singleParts ? `已编辑 ${singleParts.name}` : `已编辑 ${count} 个文件`}
            </span>
            <span className="turn-diff-sub">
              {summary.total_additions > 0 || summary.total_deletions > 0 ? (
                <span className="turn-diff-stats">
                  <span className="diff-add">+{summary.total_additions}</span>
                  <span className="diff-del">-{summary.total_deletions}</span>
                </span>
              ) : null}
              {isSingle && (
                <span className="turn-diff-link-hint">
                  查看变更
                  <Icon name="external" size={11} />
                </span>
              )}
              {reverted && <span className="turn-diff-reverted-tag">已撤销</span>}
            </span>
          </span>
        </button>

        <div className="turn-diff-actions">
          {canRevert && !reverted && (
            confirming ? (
              <div className="turn-diff-confirm">
                <button
                  type="button"
                  className="turn-diff-btn danger"
                  disabled={reverting}
                  onClick={() => void handleRevert()}
                >
                  {reverting ? "撤销中…" : "确认撤销"}
                </button>
                <button
                  type="button"
                  className="turn-diff-btn ghost"
                  disabled={reverting}
                  onClick={() => setConfirming(false)}
                >
                  取消
                </button>
              </div>
            ) : (
              <button
                type="button"
                className="turn-diff-btn ghost"
                disabled={reverting}
                onClick={() => setConfirming(true)}
                title="将本轮修改的文件恢复到修改前状态"
              >
                撤销
                <Icon name="reset" size={13} />
              </button>
            )
          )}
          <button
            type="button"
            className="turn-diff-btn primary"
            onClick={() => onViewDiff(summary.files[0]?.path)}
          >
            查看变更
          </button>
        </div>
      </div>

      {!isSingle && (
        <div className="turn-diff-files" role="list">
          {summary.files.map(file => {
            const { dir, name } = splitPath(file.path);
            const selected = active && activeFile === file.path;
            return (
              <button
                key={file.path}
                type="button"
                role="listitem"
                className={`turn-diff-file-row ${selected ? "selected" : ""}`}
                onClick={() => onViewDiff(file.path)}
                title={file.path}
              >
                <span className="turn-diff-file-path">
                  {dir && <span className="turn-diff-file-dir">{dir}</span>}
                  <span className="turn-diff-file-name">{name}</span>
                </span>
                <span className="turn-diff-file-stats">
                  {file.additions > 0 && <span className="diff-add">+{file.additions}</span>}
                  {file.deletions > 0 && <span className="diff-del">-{file.deletions}</span>}
                  {file.additions === 0 && file.deletions === 0 && (
                    <span className="diff-neutral">{file.status || "M"}</span>
                  )}
                </span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
