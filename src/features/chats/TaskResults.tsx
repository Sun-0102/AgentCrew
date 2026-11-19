import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import Dialog from "../../components/Dialog";
import Icon from "../../components/Icon";
import MessageBody from "./MessageBody";
import { desktop, statusLabels, taskLabels, type GroupTask, type TaskResult, type TaskSummary, type WorkspaceEvidence } from "./api";

const name = (agent: string) => agent === "user" ? "我" : agent === "codex" ? "Codex" : agent === "deepseek" ? "DeepSeek" : agent;
const date = (value: string | null) => value ? new Date(value).toLocaleString("zh-CN") : "未记录";
const stepLabel = (value: string) => statusLabels[value] ?? value;
const inputLabels: Record<string, string> = { queued: "待处理", selected: "待发送", delivered: "已投递", cancelled: "已取消" };

function Files({ evidence, label }: { evidence: WorkspaceEvidence | null; label: string }) {
  if (!evidence) return <p className="panel-caption">{label}未采集。不能据此确认结果。</p>;
  return <section className="result-files"><h3>{label}</h3><p className="result-meta">{new Date(evidence.captured_at_ms).toLocaleString("zh-CN")}</p><code className="result-path">{evidence.root}</code>
    <ul className="result-limitations">{evidence.limitations.map((item, index) => <li key={index}>{item}</li>)}</ul>
    {evidence.files.length ? evidence.files.map(file => <details className="result-detail" key={file.path}><summary><Icon name="file" size={16} /><span>{file.path}</span><small>{file.state === "captured" ? "已采集" : file.state === "truncated" ? "部分摘录" : "未采集"}</small></summary><div className="result-detail-body"><p className="result-meta">{file.bytes !== null && `${file.bytes} 字节 · `}{file.note ?? "采集时文本，不代表本任务新增或修改"}</p>{file.text !== null && <pre>{file.text}</pre>}</div></details>) : <p className="panel-caption">未采集到文件候选。请查看限制说明和 Agent 的实际文件读取记录。</p>}
    <details className="result-detail"><summary>Git 状态与差异</summary><div className="result-detail-body"><h4>HEAD</h4><pre>{evidence.head}</pre><h4>状态</h4><pre>{evidence.git_status || "未发现状态条目"}</pre><h4>相对 HEAD 差异（可能含原有修改）</h4><pre>{evidence.diff || "未发现已跟踪文件差异"}</pre></div></details>
  </section>;
}

interface Props {
  groupId: string; latestTask: GroupTask | null | undefined; acting: boolean;
  members: string[]; onClose: () => void; notify: (text: string) => void;
  onAction: (action: string) => Promise<void>;
}

export default function TaskResults({ groupId, latestTask, acting, members, onClose, notify, onAction }: Props) {
  const [tasks, setTasks] = useState<TaskSummary[]>([]);
  const [selected, setSelected] = useState(latestTask?.id ?? "");
  const [data, setData] = useState<TaskResult | null>(null);
  const [tab, setTab] = useState<"result" | "handoffs" | "files">("result");
  const [stepId, setStepId] = useState<number | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [loading, setLoading] = useState(true);
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const stamp = `${latestTask?.id}:${latestTask?.status}:${latestTask?.turn_count}:${latestTask?.queued_count}`;
  useEffect(() => {
    let active = true;
    setLoading(true); setError(null); setData(null);
    void (async () => {
      try {
        if (!desktop) { if (active) setTasks([]); return; }
        const list = await invoke<TaskSummary[]>("group_task_list", { groupId });
        if (!active) return;
        setTasks(list);
        const id = selected || list[0]?.id;
        if (!id) return;
        if (!selected) setSelected(id);
        const result = await invoke<TaskResult>("group_task_result", { groupId, taskId: id });
        if (active) setData(result);
      } catch (cause) { if (active) setError(String(cause)); }
      finally { if (active) setLoading(false); }
    })();
    return () => { active = false; };
  }, [groupId, selected, stamp, refresh]);

  async function exportResult(format: "json" | "markdown") {
    if (!data || exporting || loading) return;
    const taskId = data.task.id;
    setExporting(true); setError(null);
    try {
      const extension = format === "json" ? "json" : "md";
      const path = await save({ title: "导出任务记录（请选择新的文件名）", defaultPath: `AI-gent-${taskId}.${extension}`, filters: [{ name: format === "json" ? "JSON 工作记录" : "Markdown 工作记录", extensions: [extension] }] });
      if (!path) return;
      await invoke("group_task_export", { groupId, taskId, path, format });
      notify("任务记录已导出");
    } catch (cause) { setError(String(cause)); }
    finally { setExporting(false); }
  }

  async function act(action: string) {
    setError(null);
    try { await onAction(action); setRefresh(value => value + 1); }
    catch (cause) { setError(String(cause)); }
  }

  const latest = data?.task.id === latestTask?.id;
  const reviewable = latest && !!latestTask && ["idle", "awaiting_review"].includes(latestTask.status);
  const selectedStep = data?.steps.find(step => step.id === stepId) ?? data?.steps.at(-1);
  const finalStep = data ? [...data.steps].reverse().find(step => step.messages.some(message => message.role === "assistant")) : undefined;
  const reply = finalStep?.messages.filter(message => message.role === "assistant").map(message => message.text).join("\n\n");

  return <Dialog title="任务结果" onClose={onClose} wide><div className="task-results">
    <div className="result-toolbar"><label><span>历史任务 · 最近 100 项</span><select value={selected} disabled={loading && !tasks.length} aria-label="选择历史任务" onChange={event => { setSelected(event.target.value); setStepId(null); }}>{!tasks.length && <option value="">暂无任务</option>}{tasks.map(task => <option value={task.id} key={task.id}>{date(task.created_at)} · {task.objective.replace(/\s+/g, " ").slice(0, 65)}</option>)}</select></label><button type="button" className="outline-button" disabled={loading || !desktop} onClick={() => setRefresh(value => value + 1)}>刷新</button></div>
    {error && <p className="inline-error" role="alert">{error}</p>}
    {loading ? <p className="panel-caption" role="status">正在读取任务记录…</p> : !data ? <p className="panel-caption">{desktop ? "本群还没有任务记录。发送任务后可在此查看。" : "请在桌面版查看本机任务记录。"}</p> : <>
      <div className="result-overview"><div><span className="result-eyebrow">{taskLabels[data.task.status] ?? data.task.status}</span><h3>{data.task.objective}</h3><p className="result-meta">{data.task.turn_count} 次执行 · {data.steps.length} 轮记录 · 更新于 {date(data.task.updated_at)}</p></div></div>
      <div className="result-tabs" role="tablist" aria-label="任务内容">{([["result", "结果"], ["handoffs", "交接与复核"], ["files", "文件依据"]] as const).map(([id, label]) => <button type="button" role="tab" id={`result-tab-${id}`} aria-selected={tab === id} aria-controls="task-result-content" key={id} className={tab === id ? "active" : ""} onClick={() => setTab(id)}>{label}</button>)}</div>
      <div className="result-content" id="task-result-content" role="tabpanel" aria-labelledby={`result-tab-${tab}`}>
        {tab === "result" && <>
          <div className="result-note"><strong>{data.task.status === "completed" ? "用户已确认完成" : "尚未由用户验收"}</strong><p>{data.task.note ?? "继续发言沿用本任务。检查结果后可确认完成，或结束任务而不验收。"}</p><p>Agent 的完成说明不代表测试通过。文件复核是否充分，仍需结合实际读取记录判断。</p></div>
          <h3>{finalStep ? `${name(finalStep.agent)} · 第 ${finalStep.ordinal} 轮回复` : "暂无归档回复"}</h3>
          {reply ? <MessageBody text={reply} /> : <p className="panel-caption">当前轮次未归档，或这是新增记录功能前的任务。聊天原记录仍保留，不补造历史文件依据。</p>}
          {reviewable && <div className="result-review-actions"><span>按需发起实际文件复核</span>{(["codex", "deepseek"] as const).filter(agent => members.includes(agent) && agent !== finalStep?.agent).map(agent => <button type="button" className="outline-button" disabled={acting || !latestTask?.[agent === "codex" ? "codex_model" : "deepseek_model"]} key={agent} onClick={() => void act(`review_${agent}`)}>请 {name(agent)} 复核</button>)}<button type="button" className="outline-button" disabled={acting} onClick={() => void act("close")}>结束任务，不验收</button><button type="button" className="primary" disabled={acting} onClick={() => void act("accept")}>确认完成</button><small>继续发言会沿用本任务。复核使用本任务模型与现有执行权限，并发送只读检查要求。</small></div>}
          <details className="result-detail"><summary>记录范围与未验证限制</summary><ul className="result-limitations">{data.limitations.map((item, index) => <li key={index}>{item}</li>)}</ul></details>
        </>}
        {tab === "handoffs" && <>
          <p className="result-meta">这里记录谁接到了什么要求。回复与工具记录分别展示，应用不自动判定复核通过。</p>
          {!data.steps.length && <p className="panel-caption">此任务没有独立轮次归档。</p>}
          {data.steps.map(step => <section className="result-step" key={step.id}><div className="result-step-heading"><span className="result-step-number">{step.ordinal}</span><div><h3>{name(step.source_agent)} → {name(step.agent)}</h3><p className="result-meta">{stepLabel(step.status)} · {date(step.created_at)}{step.finished_at && ` → ${date(step.finished_at)}`}</p></div></div><p className="result-meta">来源消息：{step.source_message ?? "旧记录未保存来源"}</p><details className="result-detail"><summary>交接要求</summary><pre>{step.instruction}</pre></details>{step.messages.filter(message => message.role === "assistant").map(message => <div className="result-agent-reply" key={message.id}><small>{name(step.agent)} 陈述 · {message.id}</small><MessageBody text={message.text} /></div>)}<details className="result-detail"><summary>实际工具记录 · {step.messages.filter(message => message.role === "tool").length} 条</summary>{step.messages.filter(message => message.role === "tool").map(message => <div key={message.id}><p className="result-meta">{message.id} · {date(message.created_at)}</p><pre>{message.text}</pre></div>)}{!step.messages.some(message => message.role === "tool") && <p className="panel-caption">未捕获工具记录，不能据此确认读取过文件。</p>}</details></section>)}
          <details className="result-detail"><summary>用户指派与插话 · {data.inputs.length} 项</summary>{data.inputs.map((input, index) => <div key={`${input.message_id}:${index}`}><p className="result-meta">{name(input.target_agent)} · {inputLabels[input.status] ?? input.status} · {date(input.created_at)}</p><pre>{input.text}</pre></div>)}</details>
        </>}
        {tab === "files" && <>{data.steps.length ? <><label className="result-step-picker">查看轮次<select aria-label="选择文件依据轮次" value={selectedStep?.id ?? ""} onChange={event => setStepId(Number(event.target.value))}>{data.steps.map(step => <option key={step.id} value={step.id}>第 {step.ordinal} 轮 · {name(step.agent)} · {stepLabel(step.status)}</option>)}</select></label>{selectedStep && <><Files evidence={selectedStep.before} label="执行前文件依据" /><Files evidence={selectedStep.after} label="执行后文件依据" /></>}</> : <p className="panel-caption">旧任务没有文件快照。不会用当前文件冒充历史依据。</p>}</>}
      </div>
    </>}
    <div className="result-export"><p>导出含文件摘录和工具记录。分享前请检查敏感内容。</p><div><button type="button" className="outline-button" disabled={!data || loading || exporting} onClick={() => void exportResult("markdown")}>导出 Markdown</button><button type="button" className="outline-button" disabled={!data || loading || exporting} onClick={() => void exportResult("json")}>导出 JSON</button></div></div>
  </div></Dialog>;
}
