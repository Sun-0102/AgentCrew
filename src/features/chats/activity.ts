import { running, type Message, type Snapshot } from "./api";

export interface ReplyBlock extends Message {
  activity: Message[];
  startedAt: string | null;
  endedAt: string | null;
  outcome?: string;
  active: boolean;
}

// SQLite timestamps without an offset are UTC.
export function timestamp(value: string | null) {
  if (!value) return NaN;
  return Date.parse(/[zZ]|[+-]\d\d:\d\d$/.test(value) ? value : `${value.replace(" ", "T")}Z`);
}

export function elapsed(start: string | null, end: string | null, now: number) {
  const seconds = Math.max(0, Math.floor(((end ? timestamp(end) : now) - timestamp(start)) / 1000));
  if (!Number.isFinite(seconds)) return "耗时未知";
  return seconds < 60 ? `${seconds} 秒` : `${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`;
}

export function replyBlocks(snapshot: Snapshot, fallbackAgent: string): ReplyBlock[] {
  const blocks: ReplyBlock[] = [];
  let current: ReplyBlock | undefined;
  let startedAt: string | null = null;
  const latest = new Map<string, ReplyBlock>();
  const turns = new Map<string, ReplyBlock>();
  const pendingAssignment = new Map<string, Message>();

  for (const message of snapshot.messages) {
    const agent = message.agent ?? fallbackAgent;
    if (message.role === "task_start") {
      const assignment = pendingAssignment.get(agent);
      pendingAssignment.delete(agent);
      const block: ReplyBlock = {
        ...message,
        role: "assistant",
        agent,
        text: "",
        activity: assignment ? [assignment] : [],
        startedAt: message.created_at,
        endedAt: null,
        active: false,
      };
      blocks.push(block);
      turns.set(agent, block);
      latest.set(agent, block);
      current = undefined;
      continue;
    }
    if (message.role === "task_end") {
      const block = latest.get(agent);
      if (block) {
        block.endedAt = message.created_at;
        block.outcome = message.text;
      }
      turns.delete(agent);
      if (current?.agent === agent) current = undefined;
      continue;
    }
    if (message.role === "user" || message.role === "assignment") {
      startedAt = message.created_at;
      current = undefined;
      if (message.role === "user" && message.text.trim()) {
        blocks.push({ ...message, activity: [], startedAt: null, endedAt: null, active: false });
      } else if (message.role === "assignment" && message.text.trim()) {
        pendingAssignment.set(agent, message);
      }
      continue;
    }
    if (!["assistant", "tool", "thinking"].includes(message.role)) {
      current = undefined;
      continue;
    }
    if (!message.text.trim()) continue;
    const turn = turns.get(agent);
    if (turn) {
      turn.activity.push(message);
      continue;
    }
    if (!current || current.agent !== agent) {
      const assignment = pendingAssignment.get(agent);
      pendingAssignment.delete(agent);
      current = {
        ...message,
        role: "assistant",
        agent,
        text: "",
        activity: assignment ? [assignment] : [],
        startedAt: startedAt ?? message.created_at,
        endedAt: null,
        active: false,
      };
      blocks.push(current);
      latest.set(agent, current);
    }
    current.activity.push(message);
  }

  for (const block of blocks) {
    if (block.role === "user") continue;
    const session = snapshot.sessions?.[block.agent!] ?? (block.agent === (snapshot.agent ?? fallbackAgent) ? snapshot.session : null);
    block.active = latest.get(block.agent!) === block && !block.outcome && running(session?.status);

    const assistants = block.activity.filter(m => m.role === "assistant");
    const hasWorkLog = block.activity.some(m => m.role === "tool" || m.role === "thinking" || m.role === "assignment");
    const lastWorkIndex = block.activity.reduce(
      (last, m, index) => (m.role === "tool" || m.role === "thinking" ? index : last),
      -1,
    );
    const trailingAssistants = block.activity.slice(lastWorkIndex + 1).filter(m => m.role === "assistant");
    const lastReply = trailingAssistants.at(-1);

    if (block.active) {
      // Stream trailing assistant text live in the main reply bubble while keeping thinking & tools in the work log.
      const liveFinal = lastReply ? [lastReply] : [];
      block.text = liveFinal.map(m => m.text.trimEnd()).join("\n\n");
      block.activity = block.activity.filter(m => !liveFinal.includes(m));
    } else {
      const final = hasWorkLog
        ? assistants.filter(m => m === lastReply || /!\[[^\]]*\]\(/.test(m.text))
        : assistants;
      block.text = final.map(m => m.text.trimEnd()).join("\n\n");
      block.activity = block.activity.filter(m => !final.includes(m));
      if (!block.endedAt) {
        block.endedAt = assistants.at(-1)?.created_at ?? block.activity.at(-1)?.created_at ?? null;
      }
    }
  }
  return blocks;
}
