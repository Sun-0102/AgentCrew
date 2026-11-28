import { useEffect, useId, useMemo, useRef, useState } from "react";
import Avatar from "../../components/Avatar";
import Icon from "../../components/Icon";
import { agentName, type Model, type Probe } from "./api";
import { effortLabels, selectedEffort } from "./reasoning";
import { speedChoices } from "./speed";

interface Props {
  agent: string;
  members: string[];
  isGroup: boolean;
  probe: Probe | null;
  connecting: boolean;
  model: string;
  reasoningEffort?: string;
  serviceTier?: string;
  disabled: boolean;
  onAgent: (agent: string) => void;
  onModel: (model: string) => void;
  onReasoningEffort?: (value: string) => void;
  onServiceTier?: (value: string) => void;
  onConnect: () => void;
  canSaveAsDefault?: boolean;
  defaultScopeLabel?: string;
  onSaveAsDefault?: () => void;
}

export default function ModelConfigPopover({
  agent,
  members,
  isGroup,
  probe,
  connecting,
  model,
  reasoningEffort,
  serviceTier,
  disabled,
  onAgent,
  onModel,
  onReasoningEffort,
  onServiceTier,
  onConnect,
  canSaveAsDefault = false,
  defaultScopeLabel = "默认",
  onSaveAsDefault,
}: Props) {
  const id = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [position, setPosition] = useState({ left: 12, bottom: 72, width: 372 });

  const models = probe?.models ?? [];
  const currentModel: Model | undefined = useMemo(
    () => models.find(m => m.model === model) ?? models.find(m => m.isDefault) ?? models[0],
    [models, model],
  );

  const efforts = currentModel?.supportedReasoningEfforts ?? [];
  const activeEffort = selectedEffort(currentModel, reasoningEffort);
  const activeEffortObj = efforts.find(e => e.reasoningEffort === activeEffort);
  const activeEffortLabel = activeEffort
    ? (effortLabels[activeEffort] ?? activeEffort)
    : null;

  const tiers = speedChoices(currentModel);
  const hasFastTiers = tiers.length > 1;
  const activeTier = tiers.find(t => t.id === (serviceTier ?? "default")) ?? tiers[0];
  const isFastActive = !!serviceTier && serviceTier !== "default";

  const filteredModels = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return models;
    return models.filter(
      m => m.displayName.toLowerCase().includes(q) || m.model.toLowerCase().includes(q),
    );
  }, [models, query]);

  const showSearch = models.length > 5;

  function updatePosition() {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const width = Math.min(372, window.innerWidth - 24);
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
    if (open && showSearch) {
      const timer = setTimeout(() => searchRef.current?.focus(), 30);
      return () => clearTimeout(timer);
    }
    if (!open) setQuery("");
  }, [open, showSearch]);

  const triggerLabel = currentModel?.displayName ?? (connecting ? "正在连接…" : "选择模型");
  const showMemberTabs = isGroup && members.length > 1;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={`model-config-trigger ${open ? "is-open" : ""} ${isFastActive ? "is-fast" : ""}`}
        aria-label={`模型与参数配置：${agentName(agent)} · ${triggerLabel}`}
        aria-expanded={open}
        aria-controls={id}
        aria-haspopup="dialog"
        popoverTarget={id}
        onClick={updatePosition}
        title={`${agentName(agent)} · ${triggerLabel}${activeEffortLabel ? ` · 思考：${activeEffortLabel}` : ""}${isFastActive ? ` · ${activeTier?.name ?? "快速"}` : ""}`}
      >
        {showMemberTabs && <Avatar agent={agent} small />}
        <span className="model-config-name">{triggerLabel}</span>
        {activeEffortLabel && (
          <span className="model-config-badge">{activeEffortLabel}</span>
        )}
        {isFastActive && (
          <span className="model-config-fast-dot" aria-label="快速模式已开启">
            <Icon name="bolt" size={13} />
          </span>
        )}
        <span className="model-config-chevron">
          <Icon name="down" size={13} />
        </span>
      </button>

      <div
        ref={panelRef}
        id={id}
        popover="auto"
        role="dialog"
        aria-label="模型与推理配置"
        className="model-config-popover"
        style={{ left: position.left, bottom: position.bottom, width: position.width }}
        onToggle={event => {
          const isOpen = event.newState === "open";
          setOpen(isOpen);
          if (isOpen) updatePosition();
        }}
      >
        {showMemberTabs && (
          <div className="mcp-member-tabs" role="tablist" aria-label="选择要配置的群成员">
            {members.map(memberId => (
              <button
                key={memberId}
                type="button"
                role="tab"
                aria-selected={memberId === agent}
                className={`mcp-member-tab ${memberId === agent ? "active" : ""}`}
                onClick={() => {
                  onAgent(memberId);
                  setQuery("");
                }}
              >
                <Avatar agent={memberId} small />
                <span>{agentName(memberId)}</span>
              </button>
            ))}
          </div>
        )}

        <div className="mcp-section mcp-models-section">
          <div className="mcp-section-header">
            <span className="mcp-section-title">
              {showMemberTabs ? `${agentName(agent)} 模型` : "选择模型"}
            </span>
            {models.length > 0 && (
              <span className="mcp-section-count">{models.length} 个可用</span>
            )}
          </div>

          {showSearch && (
            <label className="mcp-search">
              <Icon name="search" size={14} />
              <input
                ref={searchRef}
                type="text"
                value={query}
                onChange={e => setQuery(e.target.value)}
                placeholder="搜索模型名称或 ID…"
                aria-label="搜索模型"
              />
              {query && (
                <button
                  type="button"
                  className="mcp-search-clear"
                  aria-label="清空搜索"
                  onClick={() => setQuery("")}
                >
                  <Icon name="close" size={12} />
                </button>
              )}
            </label>
          )}

          {!probe ? (
            <div className="mcp-empty">
              <p>{connecting ? `正在连接 ${agentName(agent)}…` : `${agentName(agent)} 尚未连接`}</p>
              {!connecting && (
                <button
                  type="button"
                  className="outline-button"
                  onClick={() => {
                    onConnect();
                    panelRef.current?.hidePopover();
                  }}
                >
                  立即连接 {agentName(agent)}
                </button>
              )}
            </div>
          ) : filteredModels.length === 0 ? (
            <div className="mcp-empty">
              <p>没有匹配 “{query}” 的模型</p>
            </div>
          ) : (
            <div className="mcp-model-list" role="listbox" aria-label="可用模型列表">
              {filteredModels.map(item => {
                const selected = item.model === (currentModel?.model ?? model);
                const showSubId = item.model !== item.displayName;
                return (
                  <button
                    key={item.model}
                    type="button"
                    role="option"
                    aria-selected={selected}
                    disabled={disabled}
                    className={`mcp-model-option ${selected ? "selected" : ""}`}
                    onClick={() => onModel(item.model)}
                  >
                    <div className="mcp-model-option-text">
                      <span className="mcp-model-option-name">
                        {item.displayName}
                        {item.isDefault && <span className="mcp-default-tag">推荐</span>}
                      </span>
                      {showSubId && <small className="mcp-model-option-id">{item.model}</small>}
                    </div>
                    {selected && (
                      <span className="mcp-check">
                        <Icon name="check" size={15} />
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          )}
        </div>

        {(efforts.length > 0 || hasFastTiers) && probe && (
          <div className="mcp-section mcp-params-section">
            {efforts.length > 0 && (
              <div className="mcp-param-block">
                <div className="mcp-section-header">
                  <span className="mcp-section-title">
                    <Icon name="spark" size={13} />
                    思考程度
                  </span>
                  {activeEffort !== selectedEffort(currentModel) && (
                    <button
                      type="button"
                      className="mcp-reset-link"
                      disabled={disabled}
                      onClick={() => onReasoningEffort?.(selectedEffort(currentModel))}
                    >
                      恢复默认
                    </button>
                  )}
                </div>
                <div className="mcp-segmented" role="radiogroup" aria-label="思考程度">
                  {efforts.map(choice => {
                    const isSelected = choice.reasoningEffort === activeEffort;
                    const text = effortLabels[choice.reasoningEffort] ?? choice.reasoningEffort;
                    return (
                      <button
                        key={choice.reasoningEffort}
                        type="button"
                        role="radio"
                        aria-checked={isSelected}
                        disabled={disabled}
                        className={`mcp-seg-btn ${isSelected ? "active" : ""}`}
                        onClick={() => onReasoningEffort?.(choice.reasoningEffort)}
                      >
                        {text}
                      </button>
                    );
                  })}
                </div>
                {activeEffortObj?.description && (
                  <p className="mcp-param-desc">{activeEffortObj.description}</p>
                )}
              </div>
            )}

            {hasFastTiers && (
              <div className="mcp-param-block">
                <div className="mcp-section-header">
                  <span className="mcp-section-title">
                    <Icon name="bolt" size={13} />
                    响应速度
                  </span>
                </div>
                <div className="mcp-segmented" role="radiogroup" aria-label="响应速度">
                  {tiers.map(tier => {
                    const isSelected = tier.id === (activeTier?.id ?? "default");
                    const isAccel = tier.id !== "default";
                    return (
                      <button
                        key={tier.id}
                        type="button"
                        role="radio"
                        aria-checked={isSelected}
                        disabled={disabled}
                        className={`mcp-seg-btn ${isSelected ? (isAccel ? "active-fast" : "active") : ""}`}
                        onClick={() => onServiceTier?.(tier.id)}
                      >
                        {isAccel && <Icon name="bolt" size={12} />}
                        {tier.name}
                      </button>
                    );
                  })}
                </div>
                {activeTier?.description && (
                  <p className="mcp-param-desc">{activeTier.description}</p>
                )}
              </div>
            )}
          </div>
        )}

        {probe && onSaveAsDefault && (
          <div className="mcp-default-bar">
            {canSaveAsDefault ? (
              <>
                <span className="mcp-default-hint">仅当前会话生效</span>
                <button
                  type="button"
                  className="mcp-save-default-btn"
                  onClick={() => onSaveAsDefault()}
                >
                  设为{defaultScopeLabel}
                </button>
              </>
            ) : (
              <span className="mcp-default-synced">
                <Icon name="check" size={12} />
                已与{defaultScopeLabel}一致
              </span>
            )}
          </div>
        )}

        {disabled && probe && (
          <div className="mcp-locked-note">当前任务正在执行，完成后可修改模型与参数。</div>
        )}
      </div>
    </>
  );
}
