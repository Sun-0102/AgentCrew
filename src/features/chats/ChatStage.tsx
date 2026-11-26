import { useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent, type ClipboardEvent, type DragEvent, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../../components/Icon";
import Avatar from "../../components/Avatar";
import MessageBody from "./MessageBody";
import TaskResults from "./TaskResults";
import TaskActivity from "./TaskActivity";
import TurnDiffCard from "./TurnDiffCard";
import TurnDiffDrawer, { type FilePreviewTarget, type TurnDiffOption } from "./TurnDiffDrawer";
import { replyBlocks } from "./activity";
import ProjectAgentPicker from "./ProjectAgentPicker";
import ModelConfigPopover from "./ModelConfigPopover";
import ContextMeter from "./ContextMeter";
import { activeMention, insertMention, mentionSegments, messageRecipients } from "./mentions";
import GroupMemberStatus from "./GroupMemberStatus";
import GroupMembers from "./GroupMembers";
import QueuedMessagesBar from "./QueuedMessagesBar";
import { deleteSkillTokenAtCursor, requestedSkills, skillCommandSpans, skillPrefix, type Skill } from "./skills";
import { createTextPasteAttachment, formatAttachmentSize, isLargeTextPaste, MAX_ATTACHMENTS, prepareAttachments, type Attachment, type SavedAttachment } from "./attachments";
import { useCopyToClipboard } from "./clipboard";
import { agents, agentName, providerKind, mentionName, useAgents, clock, desktop, running, statusLabels, taskLabels, type Group, type GroupSession, type PrivateSession, type ProjectSession, type Message, type Probe, type QueuedMessage, type Scope, type Snapshot } from "./api";
import type { Project } from "../projects/api";

interface Props {
  scope: Scope; scopeKey: string; title: string; group?: Group; project?: Project; agent?: string;
  snapshot: Snapshot; probe: Probe | null; model: string; connecting: boolean;
  onAgent: (agent: string) => void;
  collaborationModels: Record<string, string>;
  codexServiceTier?: string; onServiceTier?: (value: string) => void;
  reasoningEffort?: string; collaborationEfforts?: Record<string, string>; collaborationTiers?: Record<string, string>;
  onReasoningEffort?: (value: string) => void;
  onModel: (model: string) => void; onConnect: () => void; onInvite: () => void; onSettings: () => void;
  onRefresh: () => Promise<void>; notify: (text: string) => void;
  session?: GroupSession | PrivateSession | ProjectSession; onGroups?: () => void; onRenameSession?: () => void; onArchiveSession?: () => void; sessionSaving?: boolean;
  onDeleteSession?: () => void; onGroupUpdated?: (group: Group) => void;
  sidebarControls?: ReactNode;
  onOpenGit?: (path?: string) => void;
}
interface Reply { author: string; text: string }

function MessageCopyButton({ text, onNotify }: { text: string; onNotify: (message: string) => void }) {
  const { copied, copy } = useCopyToClipboard({
    onSuccess: () => onNotify("已复制消息"),
    onError: () => onNotify("复制失败，请选择文字后手动复制"),
  });
  return (
    <button
      type="button"
      className={copied ? "is-copied" : undefined}
      disabled={!text}
      onClick={() => void copy(text)}
      aria-label={copied ? "已复制消息" : "复制消息"}
      title={copied ? "已复制" : "复制消息"}
    >
      <Icon name={copied ? "check" : "copy"} size={14} />
    </button>
  );
}
const drafts = new Map<string, { text: string; files: Attachment[]; reply: Reply | null }>();
export function discardSessionDraft(session: GroupSession | PrivateSession | ProjectSession) {
  const key = "group_id" in session
    ? (session.id === session.group_id ? `group:${session.group_id}` : `group:${session.group_id}:${session.id}`)
    : "project_id" in session
      ? (session.is_default ? `project:${session.project_id}` : `project:${session.project_id}:${session.id}`)
      : (session.is_default ? `private:${session.agent_id}` : `private:${session.agent_id}:${session.id}`);
  drafts.delete(key);
  localStorage.removeItem(`ai-gent.draft.${key}`);
}

export default function ChatStage(p: Props) {
  useAgents();
  const provider = providerKind(p.agent ?? "codex");
  const { scope, scopeKey, title, group, project, agent = "codex", snapshot, probe, model, connecting, onAgent, onModel, onConnect, onInvite, onSettings, onRefresh, notify } = p;
  const [text, setText] = useState(() => drafts.get(scopeKey)?.text ?? localStorage.getItem(`ai-gent.draft.${scopeKey}`) ?? "");
  const [files, setFiles] = useState<Attachment[]>(() => drafts.get(scopeKey)?.files ?? []);
  const [reply, setReply] = useState<Reply | null>(() => drafts.get(scopeKey)?.reply ?? null);
  const [sending, setSending] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [followUpMode, setFollowUpMode] = useState<"queue" | "steer">(() =>
    localStorage.getItem("ai-gent.follow-up-mode") === "steer" ? "steer" : "queue",
  );
  const [editingQueueId, setEditingQueueId] = useState<string | null>(null);
  const [optimistic, setOptimistic] = useState<(Message & { scopeKey: string; baselineCount: number })[]>([]);
  const [acting, setActing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [panel, setPanel] = useState<"members" | "evidence" | "diff" | null>(null);
  const [diffTurnId, setDiffTurnId] = useState<string>("");
  const [diffFile, setDiffFile] = useState<string | null>(null);
  const [previewFile, setPreviewFile] = useState<FilePreviewTarget | null>(null);
  const [diffRefresh, setDiffRefresh] = useState(0);
  const [resultsOpen, setResultsOpen] = useState(false);
  const [sessionMenu, setSessionMenu] = useState(false);
  const sessionMenuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!sessionMenu) return;
    const click = (event: MouseEvent) => { if (!sessionMenuRef.current?.contains(event.target as Node)) setSessionMenu(false); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") setSessionMenu(false); };
    document.addEventListener("click", click); document.addEventListener("keydown", key);
    return () => { document.removeEventListener("click", click); document.removeEventListener("keydown", key); };
  }, [sessionMenu]);
  const [mentions, setMentions] = useState(false);
  const [mentionQuery, setMentionQuery] = useState("");
  const [mentionIndex, setMentionIndex] = useState(0);
  const dismissedMentionRef = useRef<number | null>(null);
  const mentionControlRef = useRef<HTMLDivElement>(null);
  const mentionMenuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!mentions) return;
    const onPointerDown = (event: MouseEvent) => {
      const targetNode = event.target as Node;
      if (
        mentionMenuRef.current?.contains(targetNode) ||
        mentionControlRef.current?.contains(targetNode) ||
        input.current?.contains(targetNode)
      ) return;
      setMentions(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [mentions]);
  const [skillQuery, setSkillQuery] = useState<string | null>(null);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [skillsError, setSkillsError] = useState<string | null>(null);
  const [skillIndex, setSkillIndex] = useState(0);
  const skillItems = skills.filter(skill => !skillQuery || `${skill.name} ${skill.description} ${skill.source}`.toLowerCase().includes(skillQuery.toLowerCase()));
  const task = snapshot.task;
  const taskActive = !!task && ["running", "cancelling"].includes(task.status);
  const taskControllable = !!task && ["running", "paused", "cancelling"].includes(task.status);
  const target = agent;
  const executingAgent = snapshot.agent ?? agent;
  const feed = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const highlight = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const nearBottom = useRef(true);
  const members = useMemo(() => group?.members ?? [agent], [group?.members, agent]);
  const mentionItems = (() => {
    const q = mentionQuery.trim().toLowerCase();
    if (!q) return members;
    const score = (id: string) => {
      const name = agentName(id).toLowerCase();
      const kind = providerKind(id).toLowerCase();
      const lid = id.toLowerCase();
      if (name.startsWith(q) || lid.startsWith(q) || kind.startsWith(q)) return 0;
      if (name.includes(q) || lid.includes(q) || kind.includes(q)) return 1;
      return 2;
    };
    return members
      .map(id => ({ id, rank: score(id) }))
      .filter(item => item.rank < 2)
      .sort((a, b) => a.rank - b.rank)
      .map(item => item.id);
  })();
  const mentionsOpen = mentions && mentionItems.length > 0;
  const activeMentionIndex = Math.min(mentionIndex, Math.max(0, mentionItems.length - 1));
  const busy = sending || running(snapshot.session?.status) || Object.values(snapshot.sessions ?? {}).some(session => running(session?.status)) || taskActive;
  const composingBlocked = !!p.session?.archived || sending || (group ? task?.status === "cancelling" : false);
  const queuedMessages = useMemo(() => (group ? [] : (snapshot.queued_messages ?? [])), [group, snapshot.queued_messages]);
  const pausedQueueCount = queuedMessages.filter(item => item.status === "paused").length;
  const canSteer = !group && provider === "codex" && snapshot.session?.status === "running";
  const activeFollowUpMode: "queue" | "steer" = canSteer ? followUpMode : "queue";
  useEffect(() => {
    setEditingQueueId(null);
  }, [scopeKey]);
  useEffect(() => {
    if (editingQueueId && !queuedMessages.some(item => item.id === editingQueueId)) {
      setEditingQueueId(null);
    }
  }, [editingQueueId, queuedMessages]);
  function updateFollowUpMode(next: "queue" | "steer") {
    setFollowUpMode(next);
    try {
      localStorage.setItem("ai-gent.follow-up-mode", next);
    } catch {
      // ignore storage errors
    }
  }
  function startEditQueued(item: QueuedMessage) {
    setEditingQueueId(item.id);
    setText(item.text);
    setFiles([]);
    setReply(null);
    requestAnimationFrame(() => {
      input.current?.focus();
      input.current?.setSelectionRange(item.text.length, item.text.length);
    });
  }
  function cancelEditQueued() {
    setEditingQueueId(null);
    setText("");
  }
  const recipients = group ? messageRecipients(text, members) : [agent];
  const { segments: mentionParts, matched: matchedMentions } = mentionSegments(text, members);
  const skillSpans = useMemo(() => skillCommandSpans(text), [text]);
  const hasComposerHighlight = matchedMentions.length > 0 || skillSpans.length > 0;
  const composerParts = useMemo(() => {
    if (!hasComposerHighlight) return [];
    const parts: Array<{ text: string; kind: "plain" | "mention" | "skill"; provider?: string }> = [];
    let offset = 0;
    for (const segment of mentionParts) {
      const segStart = offset;
      const segEnd = offset + segment.text.length;
      offset = segEnd;
      if (segment.agentId) {
        parts.push({ text: segment.text, kind: "mention", provider: segment.provider });
        continue;
      }
      const overlapping = skillSpans.filter(span => span.start >= segStart && span.end <= segEnd);
      if (!overlapping.length) {
        parts.push({ text: segment.text, kind: "plain" });
        continue;
      }
      let localCursor = 0;
      for (const span of overlapping) {
        const localStart = span.start - segStart;
        const localEnd = span.end - segStart;
        if (localStart > localCursor) {
          parts.push({ text: segment.text.slice(localCursor, localStart), kind: "plain" });
        }
        parts.push({ text: segment.text.slice(localStart, localEnd), kind: "skill" });
        localCursor = localEnd;
      }
      if (localCursor < segment.text.length) {
        parts.push({ text: segment.text.slice(localCursor), kind: "plain" });
      }
    }
    return parts;
  }, [hasComposerHighlight, mentionParts, skillSpans]);
  const mentionTone = matchedMentions.length > 1 ? "multi" : matchedMentions.length === 1 ? providerKind(matchedMentions[0]) : "";
  const namedTarget = recipients.length === 1 ? recipients[0] : agent;
  const recipient = namedTarget;
  const ready = group ? recipients.every(id => members.includes(id) && !!p.collaborationModels[id as string]) : !!probe && (!probe.requires_auth || !!probe.auth) && !!model;
  const userMessageCount = snapshot.messages.filter(m => m.role === "user").length;
  const pendingMessages = useMemo(
    () => optimistic.filter(item => item.scopeKey === scopeKey && userMessageCount <= item.baselineCount),
    [optimistic, scopeKey, userMessageCount],
  );
  const visibleMessages = useMemo(
    () => replyBlocks({ ...snapshot, messages: [...snapshot.messages, ...pendingMessages] }, agent),
    [snapshot, pendingMessages, agent],
  );
  const turnDiffOptions: TurnDiffOption[] = useMemo(() => {
    const withDiff = visibleMessages.filter(m => m.role === "assistant" && m.turnDiff);
    return withDiff.map((m, idx) => ({
      id: m.id,
      label:
        idx === withDiff.length - 1
          ? `最新一轮 (${m.turnDiff!.files.length} 个文件)`
          : `第 ${idx + 1} 轮 (${m.turnDiff!.files.length} 个文件)`,
      summary: m.turnDiff!,
    }));
  }, [visibleMessages]);
  const records = useMemo(
    () => snapshot.messages.filter(m => !["user", "assistant", "turn_diff", "task_start", "task_end"].includes(m.role)),
    [snapshot.messages],
  );
  const openTurnDiff = useCallback((turnId: string, filePath?: string) => {
    setPreviewFile(null);
    setDiffTurnId(turnId);
    setDiffFile(filePath ?? null);
    setPanel("diff");
  }, []);
  const openFilePreview = useCallback((filePath: string, line?: number) => {
    setPreviewFile({ path: filePath, line });
    setPanel("diff");
  }, []);
  const hasMessages = visibleMessages.length > 0;
  const providerWorking = running(snapshot.session?.status);
  const attention = !!task?.members?.some(member => member.error) || !!error || !!snapshot.session?.error || !!probe?.sidebar_warning || !!task && ["blocked", "interrupted"].includes(task.status);
  const stateText = error ? "操作未完成" : task?.status === "cancelling" ? "正在停止" : snapshot.approvals.length ? `${agentName(snapshot.approvals[0]?.agent_id ?? agent)} 等待你批准` : providerWorking ? `${agentName(executingAgent)} ${statusLabels[snapshot.session?.status ?? "running"] ?? "正在处理"}${task?.status === "paused" ? " · 结束后暂停" : ""}` : sending ? "正在发送" : task ? task.status === "running" ? `${agentName(task.current_agent)} 正在处理` : taskLabels[task.status] ?? task.status : snapshot.session ? statusLabels[snapshot.session.status] ?? "就绪" : connecting ? "正在连接" : ready ? "就绪" : "尚未连接";
  useEffect(() => {
    drafts.set(scopeKey, { text, files, reply });
    const timer = window.setTimeout(() => {
      try {
        if (text) localStorage.setItem(`ai-gent.draft.${scopeKey}`, text);
        else localStorage.removeItem(`ai-gent.draft.${scopeKey}`);
      } catch {
        setError("草稿未能保存，请检查本机存储空间。");
      }
    }, 250);
    return () => window.clearTimeout(timer);
  }, [scopeKey, text, files, reply]);
  useEffect(() => { if (feed.current && (!hasMessages || nearBottom.current)) feed.current.scrollTop = hasMessages ? feed.current.scrollHeight : 0; }, [hasMessages, pendingMessages.length, snapshot.messages.map(m => `${m.id}:${m.text.length}`).join("|")]);
  useEffect(() => {
    if (!group || composingBlocked) return;
    if (members.includes(recipient) && recipient !== agent) onAgent(recipient);
  }, [recipient, agent, members.join(","), composingBlocked]);
  async function addFiles(items: File[]) {
    if (!items.length) return;
    try {
      const incoming = await prepareAttachments(items, files.length);
      setFiles(current => [...current, ...incoming]);
      setError(null);
    } catch (cause) { setError(String(cause)); }
  }
  async function attach(event: ChangeEvent<HTMLInputElement>) {
    await addFiles(Array.from(event.target.files ?? []));
    event.target.value = "";
  }
  function handlePaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    if (composingBlocked) return;
    const clipboard = event.clipboardData;
    if (!clipboard) return;
    const pastedFiles = clipboard.files.length
      ? Array.from(clipboard.files)
      : Array.from(clipboard.items).filter(item => item.kind === "file").map(item => item.getAsFile()).filter((f): f is File => !!f);
    const plainText = clipboard.getData("text/plain");
    if (pastedFiles.length) {
      if (!plainText) event.preventDefault();
      void addFiles(pastedFiles);
      return;
    }
    if (plainText && isLargeTextPaste(plainText)) {
      event.preventDefault();
      if (files.length >= MAX_ATTACHMENTS) {
        setError(`一次最多添加 ${MAX_ATTACHMENTS} 个附件。`);
        return;
      }
      try {
        const attachment = createTextPasteAttachment(plainText, files.length + 1);
        setFiles(current => [...current, attachment]);
        setError(null);
        notify("超长文本已自动折叠为文本附件");
      } catch (cause) { setError(String(cause)); }
    }
  }
  function handleDrop(event: DragEvent<HTMLFormElement>) {
    event.preventDefault();
    setDragging(false);
    if (composingBlocked) return;
    const dropped = Array.from(event.dataTransfer?.files ?? []);
    if (dropped.length) void addFiles(dropped);
  }
  const skillsOpen = skillQuery !== null;
  useEffect(() => {
    if (skillsOpen) document.getElementById(`skill-option-${Math.min(skillIndex, skillItems.length - 1)}`)?.scrollIntoView({ block: "nearest" });
  }, [skillsOpen, skillIndex, skillItems.length]);
  useEffect(() => {
    if (!skillsOpen) return;
    let disposed = false;
    setSkills([]); setSkillsError(null); setSkillsLoading(true); setSkillIndex(0);
    if (!desktop) { setSkillsLoading(false); return; }
    invoke<Skill[]>("skills_list", { ...scope, agent }).then(items => {
      if (!disposed) setSkills(items);
    }).catch(cause => { if (!disposed) setSkillsError(String(cause)); })
      .finally(() => { if (!disposed) setSkillsLoading(false); });
    return () => { disposed = true; };
  }, [scopeKey, agent, skillsOpen]);
  function updateCommands(value: string, cursor: number, opts?: { isDelete?: boolean; fromSelect?: boolean }) {
    const match = activeMention(value, cursor);
    if (!match) {
      dismissedMentionRef.current = null;
      setMentions(false);
      setMentionQuery("");
      setMentionIndex(0);
    } else {
      setMentionQuery(match.query);
      if (!opts?.fromSelect) setMentionIndex(0);
      if (opts?.isDelete) {
        // Never pop open the @ menu when deleting backwards into an existing mention.
        if (!mentions) {
          dismissedMentionRef.current = match.start;
        } else if (dismissedMentionRef.current === match.start) {
          setMentions(false);
        }
      } else if (opts?.fromSelect) {
        if (dismissedMentionRef.current === match.start) setMentions(false);
      } else {
        if (dismissedMentionRef.current !== match.start) {
          dismissedMentionRef.current = null;
          setMentions(true);
        }
      }
    }
    setSkillQuery(skillPrefix(value.slice(0, cursor))?.[1] ?? null);
    setSkillIndex(0);
  }
  function chooseSkill(skill: Skill) {
    const cursor = input.current?.selectionStart ?? text.length;
    const prefix = text.slice(0, cursor);
    const query = skillPrefix(prefix)?.[1];
    if (query === undefined) return;
    const next = prefix.slice(0, prefix.length - query.length) + skill.name + " ";
    setText(next + text.slice(input.current?.selectionEnd ?? cursor));
    setSkillQuery(null);
    requestAnimationFrame(() => { input.current?.focus(); input.current?.setSelectionRange(next.length, next.length); });
  }
  async function send(overrideMode?: "queue" | "steer") {
    if ((!text.trim() && !files.length) || composingBlocked || !ready) return;
    if (!desktop) { notify("请在桌面版中发送消息，浏览器预览不会调用 Agent"); return; }
    if (!group && editingQueueId) {
      const nextText = text.trim();
      if (!nextText) return;
      const targetId = editingQueueId;
      setSending(true); setError(null);
      try {
        await invoke("agent_queue_update", { ...scope, agent, id: targetId, text: nextText });
        setEditingQueueId(null);
        setText("");
        await onRefresh();
        notify("已更新排队消息");
      } catch (cause) {
        setError(String(cause));
      } finally {
        setSending(false);
      }
      return;
    }
    const targetAgent = recipient;
    if (!members.includes(targetAgent)) { setError("这个 Agent 不在当前群里，请先邀请成员。"); return; }
    const draftText = text;
    const draftFiles = files;
    const draftReply = reply;
    const baseText = draftText.trim() || (draftFiles.every(file => file.isImage) ? "请查看以下图片：" : "请查看以下附件：");
    const singleFollowUpMode: "queue" | "steer" | null = !group && busy
      ? (overrideMode === "steer" && !canSteer ? "queue" : (overrideMode ?? activeFollowUpMode))
      : null;
    const showOptimistic = singleFollowUpMode !== "queue";
    let optimisticContent = baseText;
    if (draftReply) optimisticContent = `> 回复 ${draftReply.author}：${draftReply.text.replace(/\n/g, " ").slice(0, 500)}\n\n${optimisticContent}`;
    for (const file of draftFiles) {
      optimisticContent += file.isImage && file.previewUrl
        ? `\n\n![${file.name}](${file.previewUrl})`
        : `\n\n附件：${file.name}（${formatAttachmentSize(file.size)}）`;
    }
    const optimisticId = `optimistic-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const baselineCount = userMessageCount;
    setText(""); setFiles([]); setReply(null); setSkillQuery(null); setMentions(false);
    if (showOptimistic) {
      setOptimistic(current => [...current, { scopeKey, baselineCount, id: optimisticId, role: "user", text: optimisticContent, created_at: new Date().toISOString(), agent: targetAgent }]);
    }
    setSending(true); setError(null); nearBottom.current = true;
    try {
      let content = baseText;
      if (draftReply) content = `> 回复 ${draftReply.author}：${draftReply.text.replace(/\n/g, " ").slice(0, 500)}\n\n${content}`;
      for (const file of draftFiles) {
        const saved = await invoke<SavedAttachment>("attachment_save", { name: file.name, mimeType: file.mimeType, data: file.dataBase64 });
        content += saved.is_image
          ? `\n\n![${saved.name}](ai-gent-attachment:${saved.id})`
          : `\n\n附件：${saved.name}（${formatAttachmentSize(saved.size)}）\n文件路径：${saved.path}`;
      }
      const names = requestedSkills(draftText);
      if (names.length) content += await invoke<string>("skills_prepare", { ...scope, agents: recipients, names });
      if (group) {
        await invoke("agent_group_send", { groupId: scope.groupId, targetAgent, models: p.collaborationModels, efforts: p.collaborationEfforts ?? {}, tiers: p.collaborationTiers ?? {}, text: content });
      } else {
        await invoke("agent_send", {
          ...scope,
          agent,
          model,
          serviceTier: provider === "codex" ? p.codexServiceTier ?? "default" : null,
          reasoningEffort: p.reasoningEffort || null,
          followUpMode: singleFollowUpMode,
          text: content,
        });
      }
      await onRefresh();
      if (showOptimistic) setOptimistic(current => current.filter(item => item.id !== optimisticId));
    }
    catch (cause) {
      if (showOptimistic) setOptimistic(current => current.filter(item => item.id !== optimisticId));
      setText(current => current || draftText);
      setFiles(current => current.length ? current : draftFiles);
      setReply(current => current ?? draftReply);
      const message = String(cause).replace(/^Error:\s*/, "");
      setError(message);
      notify(`发送失败：${message}`);
    } finally { setSending(false); }
  }
  async function action(command: string, args: Record<string, unknown> = {}, reportFailure = false) {
    setActing(true); setError(null);
    try { await invoke(command, { ...scope, ...args }); await onRefresh(); }
    catch (cause) { setError(String(cause)); if (reportFailure) throw cause; } finally { setActing(false); }
  }
  function author(message: Message) { return message.role === "user" ? "我" : agentName(message.agent ?? agent); }
  function quote(message: Message) { setReply({ author: author(message), text: message.text }); input.current?.focus(); }
  function editText(event: ChangeEvent<HTMLTextAreaElement>) {
    const field = event.currentTarget;
    const inputType = (event.nativeEvent as InputEvent | undefined)?.inputType ?? "";
    const isDelete = inputType.startsWith("delete") || field.value.length < text.length;
    setText(field.value);
    updateCommands(field.value, field.selectionStart, { isDelete });
  }
  function chooseMention(id: string) {
    const name = mentionName(id);
    const field = input.current;
    const next = insertMention(text, field?.selectionStart ?? text.length, field?.selectionEnd ?? text.length, name);
    dismissedMentionRef.current = null;
    setText(next.text);
    setMentions(false);
    setMentionQuery("");
    setMentionIndex(0);
    setSkillQuery(null);
    requestAnimationFrame(() => {
      input.current?.focus();
      input.current?.setSelectionRange(next.cursor, next.cursor);
    });
  }
  return <section className={`chat-stage ${p.session ? "session-chat" : ""}`} aria-label={title}>
    {resultsOpen && group && scope.groupId && <TaskResults groupId={scope.groupId} latestTask={task} acting={acting} members={members} onClose={() => setResultsOpen(false)} notify={notify} onAction={async value => { if (!task) throw new Error("任务不存在，请刷新后重试。"); await action("group_task_control", { taskId: task.id, action: value }, true); }} />}
    {p.session && group ? <header className="chat-header session-chat-header"><div className="session-chat-lead">{p.sidebarControls}<div className="session-chat-heading"><div className="session-breadcrumb"><button onClick={p.onGroups}><Icon name="group" size={12} />所有群聊</button><span>/</span><span>{group.name}</span><Icon name="right" size={12} /><span>{project?.name ?? "群专用目录"}</span></div><h1 title={title}><span className="session-heading-name">{title}</span>{p.session.archived && <span className="session-archived-badge">已归档</span>}</h1></div></div><div className="chat-header-actions">{p.onOpenGit && <button type="button" className="outline-button" onClick={() => p.onOpenGit?.(project?.primary_root ?? snapshot.session?.workspace_path ?? undefined)} title="查看代码变更与 Git (⌘G)"><Icon name="diff" size={16} />代码变更</button>}<button className={`members-trigger ${panel === "members" ? "selected" : ""}`} onClick={() => setPanel(panel === "members" ? null : "members")} aria-label="查看聊天成员"><span className="stacked-avatars">{members.map(id => <Avatar agent={id} small key={id} />)}</span>{members.length} 位 Agent · 你</button><div className="session-menu-control" ref={sessionMenuRef}><button className="icon-button" aria-label="会话操作" aria-expanded={sessionMenu} aria-controls="session-actions" onClick={() => setSessionMenu(!sessionMenu)}><Icon name="more" size={18} /></button>{sessionMenu && <div className="session-actions" id="session-actions"><button onClick={() => { setSessionMenu(false); p.onRenameSession?.(); }}>重命名会话</button><button disabled={p.sessionSaving || taskControllable} title={taskControllable ? "请先完成或停止当前任务" : undefined} onClick={() => { setSessionMenu(false); p.onArchiveSession?.(); }}>{p.session.archived ? "恢复会话" : "归档会话"}</button><button className="destructive-action" disabled={p.sessionSaving || taskControllable} onClick={() => { setSessionMenu(false); p.onDeleteSession?.(); }}>删除会话</button><hr /><button onClick={() => { setSessionMenu(false); setResultsOpen(true); }}>任务结果</button><button onClick={() => { setSessionMenu(false); onInvite(); }}>群设置</button><button onClick={() => { setSessionMenu(false); setPanel(panel === "evidence" ? null : "evidence"); }}>聊天详情</button><button onClick={() => { setSessionMenu(false); onSettings(); }}>连接与设置</button></div>}</div></div></header> : <header className="chat-header"><div className="chat-title">{p.sidebarControls}<Avatar agent={group ? "group" : agent} /><div><h1 title={title}>{title}{!group && <span className="node-badge">本地节点</span>}{p.session?.archived && <span className="session-archived-badge">已归档</span>}</h1><p><span className={`presence-dot ${ready ? "connected" : ""}`} />{group ? project ? `关联项目 · ${project.name}` : "本机协作群 · 独立上下文" : project ? project.name : `${agentName(agent)} · 独立会话`}</p></div></div><div className="chat-header-actions">{p.onOpenGit && (project || snapshot.session?.workspace_path) && <button type="button" className="outline-button" onClick={() => p.onOpenGit?.(project?.primary_root ?? snapshot.session?.workspace_path ?? undefined)} title="查看代码变更与 Git (⌘G)"><Icon name="diff" size={16} />代码变更</button>}<button className={`members-trigger ${panel === "members" ? "selected" : ""}`} onClick={() => setPanel(panel === "members" ? null : "members")} aria-label="查看聊天成员"><span className="stacked-avatars">{members.map(id => <Avatar agent={id} small key={id} />)}</span><span>{members.length} 位 Agent<span className="members-human"> · 你</span></span><Icon name="down" size={14} /></button>{group && <button type="button" className="outline-button task-results-trigger" onClick={() => setResultsOpen(true)}><Icon name="file" size={17} />任务结果</button>}{group && <button className="outline-button invite-button" onClick={onInvite}><Icon name="settings" size={17} />群设置</button>}<button className={`icon-button ${panel === "evidence" ? "selected" : ""}`} onClick={() => setPanel(panel === "evidence" ? null : "evidence")} aria-label="查看聊天详情" title="聊天详情"><Icon name="panel" /></button>{p.session ? <div className="session-menu-control" ref={sessionMenuRef}><button className="icon-button" aria-label="会话操作" aria-expanded={sessionMenu} aria-controls="session-actions" onClick={() => setSessionMenu(!sessionMenu)}><Icon name="more" size={18} /></button>{sessionMenu && <div className="session-actions" id="session-actions"><button onClick={() => { setSessionMenu(false); p.onRenameSession?.(); }}>重命名会话</button><button disabled={p.sessionSaving || busy} title={busy ? "请先完成或停止当前任务" : undefined} onClick={() => { setSessionMenu(false); p.onArchiveSession?.(); }}>{p.session.archived ? "恢复会话" : "归档会话"}</button><button className="destructive-action" disabled={p.sessionSaving || busy} onClick={() => { setSessionMenu(false); p.onDeleteSession?.(); }}>删除会话</button><hr /><button onClick={() => { setSessionMenu(false); onSettings(); }}>连接与设置</button></div>}</div> : <button className="icon-button" aria-label="连接与设置" onClick={onSettings}><Icon name="more" /></button>}</div></header>}
    <div className={`chat-stage-body ${panel === "diff" ? "with-diff-panel" : panel ? "with-panel" : ""}`}><div className="chat-main">
      <div className="message-feed" ref={feed} onScroll={event => { const el = event.currentTarget; nearBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100; }} role="log" aria-label="聊天消息">
        {!hasMessages && <div className="conversation-welcome"><div className={`welcome-mark ${group ? "group-mark" : ""}`}>{group ? <Icon name="group" size={37} /> : <Icon name="spark" size={37} />}</div><h2>{p.session ? "一个需求，一段新对话" : group ? "围绕一个目标，一起讨论。" : "今天，想一起完成什么？"}</h2><p>{p.session ? p.session.archived ? "这个会话已归档，恢复后可继续讨论。" : "告诉成员这次要完成什么。这个会话从空上下文开始。" : group ? "直接发消息，成员会一起协调分工；@ 谁就只交给谁。" : `讨论一个想法，写一段代码，或把手头的事情交给 ${agentName(agent)}。`}</p><div className="prompt-suggestions">{(group ? ["一起梳理这个需求", "帮我检查一下方案"] : ["帮我理清一个想法", "一起开始一个新任务", "帮我检查一段代码"]).map(prompt => <button disabled={!!p.session?.archived} key={prompt} onClick={() => { setText(prompt); input.current?.focus(); }}>{prompt}<Icon name="right" size={14} /></button>)}</div></div>}
        {visibleMessages.map(message => <article className={`message-row ${message.role === "user" ? "outgoing" : "incoming"}`} key={message.id}><Avatar agent={message.role === "user" ? "me" : message.agent ?? agent} /><div className="message-content"><div className="message-meta"><strong>{author(message)}</strong><time>{clock(message.created_at)}</time><div className="message-actions"><button type="button" disabled={!message.text} onClick={() => quote(message)} aria-label={`回复${message.role === "user" ? "我的" : `${author(message)} 的`}消息`} title="回复"><Icon name="reply" size={14} /></button><MessageCopyButton text={message.text} onNotify={notify} /></div></div>{message.role === "assistant" && <TaskActivity block={message} />}{message.text && <div className="message-bubble"><MessageBody text={message.text.trimEnd()} members={members} onOpenFile={openFilePreview} workspacePath={project?.primary_root ?? snapshot.session?.workspace_path} onNotify={notify} /></div>}{message.role === "assistant" && message.turnDiff && <TurnDiffCard summary={message.turnDiff} active={panel === "diff" && diffTurnId === message.id} activeFile={panel === "diff" && diffTurnId === message.id ? diffFile : null} onViewDiff={filePath => openTurnDiff(message.id, filePath)} onNotify={notify} onReverted={() => setDiffRefresh(v => v + 1)} />}</div></article>)}
      </div>
      <div className="composer-area">
        <div className={`chat-status ${attention ? "needs-attention" : ""}`} role="status" aria-live="polite">
          <span className={`presence-dot ${busy ? "connected" : ""}`} /><span className="chat-status-text">{group && task?.members?.length && task.status === "running" && !error && !snapshot.approvals.length ? "群任务正在处理" : p.session?.archived ? "会话已归档，恢复后可继续讨论" : stateText}{task && task.queued_count > 0 ? ` · ${task.queued_count} 项待处理` : ""}{!group && queuedMessages.length > 0 ? ` · ${queuedMessages.length} 条排队${pausedQueueCount > 0 ? "（已暂停）" : ""}` : ""}</span>
          {(attention || snapshot.approvals.length > 0) && <button type="button" className="status-detail-button" onClick={() => setPanel("evidence")}>{snapshot.approvals.length ? "查看请求" : "查看详情"}</button>}
        </div>
        {group && (busy || taskControllable || attention) && <GroupMemberStatus snapshot={snapshot} members={members} />}
        {!group && queuedMessages.length > 0 && (
          <QueuedMessagesBar
            items={queuedMessages}
            busy={busy}
            canSteer={canSteer}
            acting={acting || sending}
            editingId={editingQueueId}
            onEdit={startEditQueued}
            onRemove={id => void action("agent_queue_remove", { agent, id })}
            onSendNow={(id, preferSteer) => void action("agent_queue_send_now", { agent, id, preferSteer })}
            onResume={() => void action("agent_queue_resume", { agent })}
            onClear={() => void action("agent_queue_clear", { agent })}
          />
        )}
        <form className={`composer ${dragging ? "dragging" : ""} ${mentionTone ? `has-mention has-mention-${mentionTone}` : ""} ${editingQueueId ? "is-editing-queue" : ""}`} onDragOver={event => { event.preventDefault(); if (!composingBlocked) setDragging(true); }} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node)) setDragging(false); }} onDrop={handleDrop} onSubmit={event => { event.preventDefault(); void send(); }}>
          {editingQueueId && (
            <div className="queue-edit-banner">
              <Icon name="edit" size={14} />
              <span>正在编辑排队消息 · Enter 保存 · Esc 取消</span>
              <button type="button" className="icon-button" aria-label="取消编辑排队消息" onClick={cancelEditQueued}>
                <Icon name="close" size={14} />
              </button>
            </div>
          )}
          {reply && <div className="reply-preview"><Icon name="reply" size={16} /><div><strong>回复 {reply.author}</strong><p>{reply.text.slice(0, 140)}</p></div><button type="button" className="icon-button" aria-label="取消回复" onClick={() => setReply(null)}><Icon name="close" size={15} /></button></div>}
          {!!files.length && <div className="attachment-list">{files.map((file, index) => <div className="attachment-chip" key={`${file.name}-${index}`}>{file.isImage && file.previewUrl ? <img className="attachment-thumb" src={file.previewUrl} alt={file.name} /> : <Icon name="file" size={18} />}<span>{file.name}<small>{formatAttachmentSize(file.size)} · {file.isImage ? "图片" : "文件"}</small></span><button type="button" aria-label={`移除附件 ${file.name}`} onClick={() => setFiles(current => current.filter((_, i) => i !== index))}><Icon name="close" size={13} /></button></div>)}</div>}
          {skillsOpen && <div className="skill-suggestions" id="skill-suggestions" role="listbox" aria-label="技能列表">
            <div className="menu-caption">{agentName(agent)} · 技能 {skillsLoading ? "" : `(${skills.length})`}</div>
            {skillsLoading ? <p role="status">正在读取本地和运行时技能…</p> : skillsError ? <p role="alert">{skillsError}</p> : !skillItems.length ? <p role="status">{desktop ? "没有匹配的技能。请检查搜索词或当前 Agent 的技能配置。" : "请在桌面版中读取本地技能。"}</p> : skillItems.map((skill, index) => <button type="button" role="option" id={`skill-option-${index}`} aria-selected={index === Math.min(skillIndex, skillItems.length - 1)} key={skill.name} disabled={composingBlocked} onMouseDown={event => event.preventDefault()} onClick={() => chooseSkill(skill)}><strong>/{skill.name}<small className="skill-source">{skill.source}</small></strong><span>{skill.description || "本地技能"}</span></button>)}
          </div>}
          {mentionsOpen && <div className="mention-menu" id="mention-menu" role="listbox" aria-label="选择要 @ 的成员" ref={mentionMenuRef}><div className="menu-caption">选择要 @ 的成员</div>{mentionItems.map((id, index) => <button type="button" role="option" id={`mention-option-${index}`} aria-selected={index === activeMentionIndex} key={id} disabled={composingBlocked} onMouseEnter={() => setMentionIndex(index)} onMouseDown={event => event.preventDefault()} onClick={() => chooseMention(id)}><Avatar agent={id} small /><span>{agentName(id)}<small>{providerKind(id)} · {id.slice(0, 8)}</small></span>{target === id && <Icon name="check" size={16} />}</button>)}</div>}
          <label className="sr-only" htmlFor="chat-input">输入消息</label>
          <div className={`composer-editor ${hasComposerHighlight ? "has-highlight" : ""}`}>
            {hasComposerHighlight && (
              <div className="composer-highlight" ref={highlight} aria-hidden="true">
                {composerParts.map((part, index) =>
                  part.kind === "mention" ? (
                    <mark key={index} className={`composer-mention is-${part.provider ?? "default"}`}>{part.text}</mark>
                  ) : part.kind === "skill" ? (
                    <mark key={index} className="composer-skill">{part.text}</mark>
                  ) : (
                    <span key={index}>{part.text}</span>
                  ),
                )}
                {text.endsWith("\n") ? "\u200b" : null}
              </div>
            )}
            <textarea id="chat-input" ref={input} value={text} onChange={editText} onPaste={handlePaste} onScroll={event => { if (highlight.current) { highlight.current.scrollTop = event.currentTarget.scrollTop; highlight.current.scrollLeft = event.currentTarget.scrollLeft; } }} onSelect={event => updateCommands(event.currentTarget.value, event.currentTarget.selectionStart, { fromSelect: true })} placeholder={p.session?.archived ? "恢复会话后可继续讨论" : editingQueueId ? "修改排队消息内容，Enter 保存，Esc 取消…" : group ? "在这个会话里继续讨论，输入 / 调用技能，或 @ 指定成员…" : busy ? (canSteer ? (activeFollowUpMode === "steer" ? "输入内容直接纠偏当前轮（Enter 纠偏 · ⌘Enter 加入排队）…" : "继续输入下一条，将自动排队等待（Enter 排队 · ⌘Enter 立即纠偏）…") : "执行中也可继续输入，发送后自动排队等待…") : "把想法写下来，输入 / 调用技能，支持粘贴或拖拽图片与文件…"} aria-expanded={skillsOpen} aria-controls={skillsOpen ? "skill-suggestions" : undefined} aria-activedescendant={skillsOpen && skillItems.length ? `skill-option-${Math.min(skillIndex, skillItems.length - 1)}` : undefined} disabled={composingBlocked} maxLength={25000} rows={3} onKeyDown={event => {
            if (event.nativeEvent.isComposing) return;
            if (event.key === "Backspace" && !skillsOpen && !event.metaKey && !event.ctrlKey && !event.altKey) {
              const field = event.currentTarget;
              const removed = deleteSkillTokenAtCursor(text, field.selectionStart, field.selectionEnd);
              if (removed) {
                event.preventDefault();
                setText(removed.text);
                updateCommands(removed.text, removed.cursor, { isDelete: true });
                requestAnimationFrame(() => {
                  input.current?.setSelectionRange(removed.cursor, removed.cursor);
                });
                return;
              }
            }
            if (editingQueueId && event.key === "Escape") {
              event.preventDefault();
              cancelEditQueued();
              return;
            }
            if (!group && !text && !editingQueueId && event.altKey && event.key === "ArrowUp" && queuedMessages.length > 0) {
              event.preventDefault();
              startEditQueued(queuedMessages[queuedMessages.length - 1]);
              return;
            }
            if (!group && busy && canSteer && !editingQueueId && !text.trim() && (event.metaKey || event.ctrlKey) && event.shiftKey && event.key === "Enter" && queuedMessages.length > 0) {
              event.preventDefault();
              void action("agent_queue_send_now", { agent, id: queuedMessages[0].id, preferSteer: true });
              return;
            }
            if (skillsOpen) {
              if (event.key === "Escape") { event.preventDefault(); setSkillQuery(null); return; }
              if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                event.preventDefault();
                setSkillIndex(index => (index + (event.key === "ArrowDown" ? 1 : -1) + Math.max(1, skillItems.length)) % Math.max(1, skillItems.length));
                return;
              }
              if ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab") {
                if (skillItems.length) { event.preventDefault(); chooseSkill(skillItems[Math.min(skillIndex, skillItems.length - 1)]); return; }
                if (event.key === "Enter") { event.preventDefault(); return; }
              }
            }
            if (mentionsOpen) {
              if (event.key === "Escape") {
                event.preventDefault();
                dismissedMentionRef.current = activeMention(text, event.currentTarget.selectionStart)?.start ?? null;
                setMentions(false);
                return;
              }
              if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                event.preventDefault();
                setMentionIndex(index => (index + (event.key === "ArrowDown" ? 1 : -1) + mentionItems.length) % mentionItems.length);
                return;
              }
              if ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab") {
                event.preventDefault();
                chooseMention(mentionItems[activeMentionIndex]);
                return;
              }
            }
            if (!group && busy && !editingQueueId && (event.metaKey || event.ctrlKey) && !event.shiftKey && event.key === "Enter") {
              event.preventDefault();
              const opposite: "queue" | "steer" = canSteer
                ? (activeFollowUpMode === "steer" ? "queue" : "steer")
                : "queue";
              void send(opposite);
              return;
            }
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              void send();
            }
          }} />
          </div>
          <div className="composer-toolbar"><div className="composer-tools"><input className="sr-only" type="file" ref={fileInput} multiple onChange={attach} tabIndex={-1} aria-label="添加图片或文件" /><button className="icon-button" type="button" title="添加图片或文件（单文件不超过 10 MB，支持粘贴和拖拽）" aria-label="添加图片或文件" disabled={composingBlocked || !!editingQueueId} onClick={() => fileInput.current?.click()}><Icon name="attach" size={20} /></button><div className="mention-control" ref={mentionControlRef}><button className={`icon-button ${mentionsOpen ? "selected" : ""}`} type="button" aria-label="指定 Agent" disabled={composingBlocked || !!editingQueueId} onClick={() => { dismissedMentionRef.current = null; setMentionQuery(""); setMentionIndex(0); setMentions(!mentions); }}><Icon name="at" size={19} /></button></div><span className="toolbar-divider" /><span className="composer-execution-controls">{scope.projectId && !group && <ProjectAgentPicker agent={agent} disabled={busy || taskControllable || !!p.session?.archived} onAgent={onAgent} onSettings={onSettings} />}<ModelConfigPopover agent={agent} members={members} isGroup={!!group} probe={probe} connecting={connecting} model={model} reasoningEffort={p.reasoningEffort} serviceTier={p.codexServiceTier} disabled={busy || taskControllable} onAgent={p.onAgent} onModel={onModel} onReasoningEffort={p.onReasoningEffort} onServiceTier={p.onServiceTier} onConnect={onConnect} /></span><ContextMeter agent={agent} members={members} isGroup={!!group} model={model} contextUsage={snapshot.context_usage} /></div><div className="composer-submit">{!probe && <button type="button" className="connect-link" onClick={onConnect} disabled={connecting}>{connecting ? "连接中…" : `连接 ${agentName(agent)}`}</button>}{!group && busy && canSteer && !editingQueueId && <div className="follow-up-toggle" role="group" aria-label="执行中发送方式"><button type="button" className={activeFollowUpMode === "queue" ? "active" : ""} onClick={() => updateFollowUpMode("queue")} title="默认加入队列，等当前轮结束后自动发送">排队</button><button type="button" className={activeFollowUpMode === "steer" ? "active is-steer" : ""} onClick={() => updateFollowUpMode("steer")} title="默认直接纠偏当前运行中的 Codex 任务 (Steer)">纠偏</button></div>}{(busy || taskControllable) && <button className="send-button stop-button" type="button" aria-label="停止任务" title="停止当前任务" disabled={acting || sending || (taskControllable ? task?.status === "cancelling" : ["starting", "cancel_requested", "stopping"].includes(snapshot.session?.status ?? "starting"))} onClick={() => taskControllable && task ? action("group_task_control", { taskId: task.id, action: "cancel" }) : action("agent_cancel", { agent: executingAgent })}><Icon name="stop" size={18} /></button>}<button className={`send-button ${!group && editingQueueId ? "queue-button" : !group && busy ? (activeFollowUpMode === "steer" ? "steer-button" : "queue-button") : ""}`} type="submit" aria-label={!group && editingQueueId ? "保存排队消息" : !group && busy ? (activeFollowUpMode === "steer" ? "纠偏当前轮" : "加入排队") : "发送消息"} title={!desktop ? "请在桌面版中发送" : !group && editingQueueId ? "保存排队消息 (Enter)" : group && busy ? "发送插话，各成员在自己的当前轮结束后处理" : !group && busy ? (canSteer ? (activeFollowUpMode === "steer" ? "纠偏当前轮 (Enter 纠偏 · ⌘Enter 加入排队)" : "加入排队等待 (Enter 排队 · ⌘Enter 立即纠偏)") : "加入排队等待，当前轮完成后自动发送") : "发送消息"} disabled={composingBlocked || !ready || (!text.trim() && !files.length) || !desktop}><Icon name={!group && editingQueueId ? "check" : !group && busy ? (activeFollowUpMode === "steer" ? "steer" : "queue") : "send"} size={19} /></button></div></div>
        </form><div className="composer-footer"><span>{p.session ? "本会话独立上下文" : group ? `发送给 ${recipients.map(id => agentName(id)).join("、")}${recipients.length > 1 ? " · 协调分工" : " · 单独处理"}` : "私聊记录独立保存"}</span><span>{!group && busy ? (<>Enter {activeFollowUpMode === "steer" ? "纠偏" : "排队"}{canSteer && <><span className="keyboard-separator">·</span>⌘Enter {activeFollowUpMode === "steer" ? "排队" : "纠偏"}</>} {queuedMessages.length > 0 && <><span className="keyboard-separator">·</span>⌥↑ 编辑排队</>}</>) : (<>Enter 发送<span className="keyboard-separator">·</span>Shift + Enter 换行</>)}</span></div>
      </div>
    </div>{panel === "diff" ? (
      <TurnDiffDrawer
        turns={turnDiffOptions}
        selectedTurnId={diffTurnId}
        selectedFile={diffFile}
        previewFile={previewFile}
        workspacePath={project?.primary_root ?? snapshot.session?.workspace_path ?? undefined}
        refreshToken={diffRefresh}
        onSelectTurn={id => {
          setPreviewFile(null);
          setDiffTurnId(id);
        }}
        onSelectFile={setDiffFile}
        onClearPreview={() => {
          setPreviewFile(null);
          if (!diffTurnId && turnDiffOptions.length > 0) {
            setDiffTurnId(turnDiffOptions[turnDiffOptions.length - 1].id);
          }
        }}
        onOpenFullGit={p.onOpenGit}
        onClose={() => {
          setPreviewFile(null);
          setPanel(null);
        }}
      />
    ) : panel && <aside className="detail-panel" aria-label={panel === "members" ? "聊天成员" : "聊天详情"}>
      <div className="panel-heading"><h2>{panel === "members" ? "聊天成员" : "聊天详情"}</h2><button className="icon-button" aria-label="关闭详情" onClick={() => setPanel(null)}><Icon name="close" size={17} /></button></div>
      {panel === "members" ? group ? <GroupMembers group={group} onUpdated={updated => { p.onGroupUpdated?.(updated); notify("群成员已更新"); }} onSettings={onInvite} /> : <><p className="panel-caption">{members.length} 位 Agent · 1 位成员</p><div className="person-row"><Avatar agent="me" small /><div><strong>我</strong><small>群主 · 本机用户</small></div><span className="availability ready">你</span></div>{members.map(id => <div className="person-row" key={id}><Avatar agent={id} small /><div><strong>{agentName(id)}</strong><small>{agents.find(a => a.id === id)?.description}</small></div><span className={`availability ${id === agent ? "ready" : ""}`}>{id === agent ? probe ? "已就绪" : "可连接" : "可指定执行"}</span></div>)}{group && <button className="outline-button panel-add" onClick={onInvite}><Icon name="settings" size={16} />群设置与项目</button>}<div className="panel-note"><Icon name="shield" /><p>这里只包含你本机的 Agent。私聊不会自动共享到群里。</p></div></> : <>
        {group && task && <div className="collaboration-card" aria-label="协作任务">
          <div className="collaboration-heading"><strong>{taskLabels[task.status] ?? task.status}</strong><span>已执行 {task.turn_count} 次{task.queued_count > 0 ? ` · ${task.queued_count} 条待处理` : ""}</span></div>
          <p className="collaboration-objective" title={task.objective}>{task.objective}</p>
          <details><summary>任务与模型</summary><p>任务：{task.id}</p><div>{task.members?.map(member=><p key={member.agent}>{agentName(member.agent)}：{member.model || "未选择模型"}</p>)}</div><p>交接和文件依据可在“任务结果”中查看。本轮回复不代表已验证通过，完成与否由你确认。</p></details>
          {task.error && <p className="collaboration-reason">{task.error}</p>}
          <div className="collaboration-actions"><button type="button" className="outline-button" onClick={() => setResultsOpen(true)}>查看任务结果</button>
            {task.status === "running" && <button type="button" className="outline-button" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "pause" })}>暂停接力</button>}
            {task.status === "paused" && <button type="button" className="outline-button" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "resume" })}>继续接力</button>}
            {["running", "paused", "cancelling"].includes(task.status) && <button type="button" className="outline-button" disabled={acting || task.status === "cancelling"} onClick={() => action("group_task_control", { taskId: task.id, action: "cancel" })}>停止</button>}
            {["idle", "awaiting_review"].includes(task.status) && <button type="button" className="primary" disabled={acting} onClick={() => action("group_task_control", { taskId: task.id, action: "accept" })}>确认完成</button>}
          </div>
        </div>}
        {snapshot.approvals.map(approval => <div className="approval-card" key={approval.id}><div><Icon name="shield" size={18} /><strong>{agentName(approval.agent_id ?? agent)} 需要你批准{approval.method.includes("fileChange") ? "文件修改" : "命令执行"}</strong></div><pre>{approval.command ?? "请检查执行记录中的文件变更。"}</pre>{approval.cwd && <small>{approval.cwd}</small>}<p>{approval.reason}</p><div className="approval-actions"><button className="outline-button" disabled={acting} onClick={() => action("agent_approve", { agent: approval.agent_id ?? agent, id: approval.id, allow: false })}>拒绝</button><button className="primary" disabled={acting} onClick={() => action("agent_approve", { agent: approval.agent_id ?? agent, id: approval.id, allow: true })}>批准本次</button></div></div>)}
        {(error || snapshot.session?.error) && <div className="inline-error" role="alert">{error ?? snapshot.session?.error}</div>}
        {probe?.sidebar_warning && <div className="sync-notice">最近列表同步未完成，私聊仍可使用。{probe.sidebar_warning}</div>}
        {provider === "gemini" && <p className="panel-caption">Gemini 使用自己的 CLI 和原厂登录，当前已开启自动批准模式（无独立沙箱隔离）。</p>}
        {provider === "claude" && <p className="panel-caption">Claude Code 使用本机 claude CLI 与原厂登录，支持流式回复、工具调用与多轮会话恢复。</p>}
        {provider === "deepseek" && <p className="panel-caption">DeepSeek 按消息更新，按当前运行时提供的工具执行任务。</p>}
        <div className="execution-status"><strong>执行详情</strong><small>{records.length} 条记录</small></div>
        {records.length ? records.map(record => <details className="panel-tool" key={record.id}>
          <summary><Icon name={record.role === "workspace" ? "folder" : record.role === "assignment" ? "at" : record.role === "thinking" ? "thought" : "terminal"} size={15} /><span>{record.role === "workspace" ? "工作目录变更" : record.role === "assignment" ? `${author(record)} · 交接详情` : record.role === "system" ? "状态记录" : record.role === "thinking" ? `${author(record)} · 思考过程` : `${author(record)} · 工具记录`}</span><Icon name="down" size={13} /></summary><pre>{record.text}</pre>
        </details>) : <p className="panel-caption">暂无执行记录。</p>}
        {(project || snapshot.session?.workspace_path) && <div className="project-context"><span>{project ? "当前项目" : "本群工作目录"}</span>{project && <strong>{project.name}</strong>}<code>{project?.primary_root ?? snapshot.session?.workspace_path}</code></div>}
      </>}
    </aside>}</div>
  </section>;
}
