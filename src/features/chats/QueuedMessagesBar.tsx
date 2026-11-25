import Icon from "../../components/Icon";
import type { QueuedMessage } from "./api";

interface Props {
  items: QueuedMessage[];
  busy: boolean;
  canSteer: boolean;
  acting: boolean;
  editingId: string | null;
  onEdit: (item: QueuedMessage) => void;
  onRemove: (id: string) => void;
  onSendNow: (id: string, preferSteer: boolean) => void;
  onResume: () => void;
  onClear: () => void;
}

export default function QueuedMessagesBar({
  items,
  busy,
  canSteer,
  acting,
  editingId,
  onEdit,
  onRemove,
  onSendNow,
  onResume,
  onClear,
}: Props) {
  if (!items.length) return null;
  const hasPaused = items.some(item => item.status === "paused");

  return (
    <div className={`queue-stack ${hasPaused ? "is-paused" : ""}`} role="region" aria-label="待发送队列">
      <div className="queue-stack-header">
        <span className="queue-stack-title">
          <Icon name="queue" size={14} />
          <span>{hasPaused ? `队列已暂停 · ${items.length} 条待发送` : `排队等待 · ${items.length} 条`}</span>
        </span>
        <div className="queue-stack-controls">
          {hasPaused && (
            <button
              type="button"
              className="queue-header-btn primary"
              disabled={acting}
              onClick={onResume}
              title="恢复队列并按顺序发送"
            >
              <Icon name="play" size={12} />
              <span>继续队列</span>
            </button>
          )}
          <button
            type="button"
            className="queue-header-btn"
            disabled={acting}
            onClick={onClear}
            title="清空所有排队消息"
          >
            清空
          </button>
        </div>
      </div>
      <div className="queue-stack-list">
        {items.map((item, index) => {
          const isEditing = editingId === item.id;
          const steerNow = busy && canSteer;
          return (
            <div key={item.id} className={`queue-item ${isEditing ? "is-editing" : ""} ${item.status === "paused" ? "is-paused" : ""}`}>
              <span className="queue-item-index">#{index + 1}</span>
              <span className="queue-item-text" title={item.text}>
                {item.text.replace(/\s+/g, " ").trim()}
              </span>
              {item.status === "paused" && <span className="queue-item-badge">已暂停</span>}
              <div className="queue-item-actions">
                <button
                  type="button"
                  className="queue-action-btn"
                  disabled={acting}
                  onClick={() => onSendNow(item.id, steerNow)}
                  aria-label={steerNow ? "立即纠偏当前轮" : busy ? "停止当前轮并立即发送" : "立即发送"}
                  title={steerNow ? "立即纠偏当前轮 (Steer)" : busy ? "停止当前轮并立即发送" : "立即发送"}
                >
                  <Icon name={steerNow ? "steer" : "send"} size={13} />
                </button>
                <button
                  type="button"
                  className="queue-action-btn"
                  disabled={acting}
                  onClick={() => onEdit(item)}
                  aria-label="编辑排队消息"
                  title="拉回输入框编辑 (⌥↑)"
                >
                  <Icon name="edit" size={13} />
                </button>
                <button
                  type="button"
                  className="queue-action-btn danger"
                  disabled={acting}
                  onClick={() => onRemove(item.id)}
                  aria-label="移除排队消息"
                  title="从队列移除"
                >
                  <Icon name="close" size={13} />
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
