import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { desktop, type Message, type Snapshot } from "./api";

export const STREAM_EVENT = "agentcrew:stream";
export const SYNC_EVENT = "agentcrew:sync";

export interface StreamDelta {
  conversation_id: string;
  item_id: string;
  role: string;
  agent?: string;
  mode: "append" | "replace";
  text: string;
}

export interface StreamBatchEvent {
  seq: number;
  deltas: StreamDelta[];
}

export type SyncScope =
  | "snapshot"
  | "groups"
  | "group_sessions"
  | "private_sessions"
  | "agents"
  | "dispatch"
  | "schedules";

export interface SyncEvent {
  seq: number;
  scopes: SyncScope[];
  conversation_id?: string;
  group_id?: string;
  agent_id?: string;
}

export interface CoalescedSync {
  scopes: Set<SyncScope>;
  conversationIds: Set<string>;
  groupIds: Set<string>;
  agentIds: Set<string>;
  resyncAll: boolean;
}

export function applyStreamDeltas(snapshot: Snapshot, deltas: StreamDelta[]): Snapshot {
  const activeConversationId = snapshot.conversation_id;
  if (!activeConversationId || !deltas.length) return snapshot;

  let messages: Message[] | null = null;
  const indexById = new Map<string, number>();

  for (const delta of deltas) {
    if (delta.conversation_id !== activeConversationId) continue;
    if (delta.mode === "append" && !delta.text) continue;

    if (!messages) {
      messages = snapshot.messages.slice();
      for (let i = 0; i < messages.length; i += 1) {
        indexById.set(messages[i].id, i);
      }
    }

    const existingIndex = indexById.get(delta.item_id);
    if (existingIndex !== undefined) {
      const current = messages[existingIndex];
      const nextText = delta.mode === "append" ? current.text + delta.text : delta.text;
      if (
        nextText === current.text &&
        current.role === delta.role &&
        (delta.agent === undefined || current.agent === delta.agent)
      ) {
        continue;
      }
      messages[existingIndex] = {
        ...current,
        role: delta.role,
        agent: delta.agent ?? current.agent,
        text: nextText,
      };
    } else {
      const created: Message = {
        id: delta.item_id,
        role: delta.role,
        text: delta.text,
        agent: delta.agent,
        created_at: new Date().toISOString(),
      };
      indexById.set(created.id, messages.length);
      messages.push(created);
    }
  }

  return messages ? { ...snapshot, messages } : snapshot;
}

export interface RealtimeHandlers {
  onStream: (deltas: StreamDelta[]) => void;
  onSync: (batch: CoalescedSync) => void;
}

const SYNC_COALESCE_MS = 40;

export function subscribeRealtimeEvents(handlers: RealtimeHandlers): () => void {
  if (!desktop) return () => {};

  let disposed = false;
  let lastSeq = 0;
  let unlistenStream: UnlistenFn | null = null;
  let unlistenSync: UnlistenFn | null = null;
  let flushTimer: ReturnType<typeof setTimeout> | null = null;

  const pending: CoalescedSync = {
    scopes: new Set<SyncScope>(),
    conversationIds: new Set<string>(),
    groupIds: new Set<string>(),
    agentIds: new Set<string>(),
    resyncAll: false,
  };

  const scheduleFlush = () => {
    if (disposed || flushTimer) return;
    flushTimer = setTimeout(() => {
      flushTimer = null;
      if (disposed) return;
      const batch: CoalescedSync = {
        scopes: new Set(pending.scopes),
        conversationIds: new Set(pending.conversationIds),
        groupIds: new Set(pending.groupIds),
        agentIds: new Set(pending.agentIds),
        resyncAll: pending.resyncAll,
      };
      pending.scopes.clear();
      pending.conversationIds.clear();
      pending.groupIds.clear();
      pending.agentIds.clear();
      pending.resyncAll = false;
      if (batch.resyncAll || batch.scopes.size > 0) {
        handlers.onSync(batch);
      }
    }, SYNC_COALESCE_MS);
  };

  const trackSeq = (seq: number) => {
    if (lastSeq > 0 && seq > lastSeq + 1) {
      pending.resyncAll = true;
      scheduleFlush();
    }
    if (seq > lastSeq) lastSeq = seq;
  };

  const triggerResyncAll = () => {
    if (disposed) return;
    pending.resyncAll = true;
    scheduleFlush();
  };

  const onVisibilityChange = () => {
    if (typeof document !== "undefined" && document.visibilityState === "visible") {
      triggerResyncAll();
    }
  };

  const onFocus = () => {
    triggerResyncAll();
  };

  if (typeof document !== "undefined") {
    document.addEventListener("visibilitychange", onVisibilityChange);
  }
  if (typeof window !== "undefined") {
    window.addEventListener("focus", onFocus);
  }

  void listen<StreamBatchEvent>(STREAM_EVENT, event => {
    if (disposed) return;
    const payload = event.payload;
    if (!payload) return;
    trackSeq(payload.seq);
    if (payload.deltas?.length) {
      handlers.onStream(payload.deltas);
    }
  }).then(unlisten => {
    if (disposed) unlisten();
    else unlistenStream = unlisten;
  });

  void listen<SyncEvent>(SYNC_EVENT, event => {
    if (disposed) return;
    const payload = event.payload;
    if (!payload) return;
    trackSeq(payload.seq);
    for (const scope of payload.scopes ?? []) {
      pending.scopes.add(scope);
    }
    if (payload.conversation_id) pending.conversationIds.add(payload.conversation_id);
    if (payload.group_id) pending.groupIds.add(payload.group_id);
    if (payload.agent_id) pending.agentIds.add(payload.agent_id);
    scheduleFlush();
  }).then(unlisten => {
    if (disposed) unlisten();
    else unlistenSync = unlisten;
  });

  return () => {
    disposed = true;
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    if (typeof document !== "undefined") {
      document.removeEventListener("visibilitychange", onVisibilityChange);
    }
    if (typeof window !== "undefined") {
      window.removeEventListener("focus", onFocus);
    }
    unlistenStream?.();
    unlistenSync?.();
  };
}
