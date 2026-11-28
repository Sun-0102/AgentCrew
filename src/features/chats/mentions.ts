import { agents } from "./api";
export interface MentionSegment {
  text: string;
  agentId?: string;
  provider?: string;
}

export function activeMention(text: string, cursor: number) {
  const match = text.slice(0, cursor).match(/(?:^|[\s，。！？、：；（(])@([^\s@]*)$/i);
  return match ? { start: cursor - match[1].length - 1, query: match[1].toLowerCase() } : null;
}

export function mentionSegments(text: string, members: string[]): { segments: MentionSegment[]; matched: string[] } {
  if (!text) return { segments: [], matched: [] };
  const memberAgents = agents.filter(a => members.includes(a.id));
  const targets = memberAgents.flatMap(agent => {
    const unique = memberAgents.filter(other => other.name.toLowerCase() === agent.name.toLowerCase()).length === 1;
    return (unique ? [agent.id, agent.name] : [agent.id]).map(label => ({
      label,
      agentId: agent.id,
      provider: agent.provider_kind,
    }));
  });
  targets.sort((a, b) => b.label.length - a.label.length);
  if (!targets.length) return { segments: [{ text }], matched: [] };

  const segments: MentionSegment[] = [];
  const matched: string[] = [];
  const pushPlain = (chunk: string) => {
    if (!chunk) return;
    const prev = segments.at(-1);
    if (prev && !prev.agentId) prev.text += chunk;
    else segments.push({ text: chunk });
  };

  let fence: { marker: string; length: number } | null = null;
  let inline: number | null = null;
  const lines = text.split("\n");

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
    const raw = lines[lineIndex];
    const suffix = lineIndex < lines.length - 1 ? "\n" : "";
    const trimmed = raw.trimStart();
    const marker = trimmed.match(/^(`{3,}|~{3,})/);
    if (marker) {
      if (!fence) fence = { marker: marker[0][0], length: marker[0].length };
      else if (fence.marker === marker[0][0] && marker[0].length >= fence.length && !trimmed.slice(marker[0].length).trim()) fence = null;
      pushPlain(raw + suffix);
      continue;
    }
    if (fence || trimmed.startsWith("文本附件：") || trimmed.startsWith(">") || raw.startsWith("    ") || raw.startsWith("\t")) {
      pushPlain(raw + suffix);
      continue;
    }

    let plainStart = 0;
    for (let cursor = 0; cursor < raw.length; cursor++) {
      if (raw[cursor] === "`") {
        const width = raw.slice(cursor).match(/^`+/)![0].length;
        if (inline === width) inline = null;
        else if (inline === null) inline = width;
        cursor += width - 1;
        continue;
      }
      if (inline !== null || raw[cursor] !== "@" || /[a-z0-9_@\\]/i.test(raw[cursor - 1] ?? "")) continue;
      const target = targets.find(t =>
        raw.slice(cursor + 1, cursor + 1 + t.label.length).toLowerCase() === t.label.toLowerCase() &&
        !/[a-z0-9_-]/i.test(raw[cursor + 1 + t.label.length] ?? ""),
      );
      if (!target) continue;
      if (cursor > plainStart) pushPlain(raw.slice(plainStart, cursor));
      const token = raw.slice(cursor, cursor + 1 + target.label.length);
      segments.push({ text: token, agentId: target.agentId, provider: target.provider });
      if (!matched.includes(target.agentId)) matched.push(target.agentId);
      cursor += target.label.length;
      plainStart = cursor + 1;
    }
    if (plainStart < raw.length) pushPlain(raw.slice(plainStart));
    if (suffix) pushPlain(suffix);
  }

  return { segments, matched };
}

export function insertMention(text: string, start: number, end: number, name: string) {
  const from = activeMention(text, start)?.start ?? start;
  const prefix = text.slice(0, from);
  const insertion = `${prefix && !/\s$/.test(prefix) ? " " : ""}@${name} `;
  return { text: prefix + insertion + text.slice(end), cursor: from + insertion.length };
}

export function messageRecipients(text: string, members: string[]) {
  const named: string[] = [];
  let fence: { marker: string; length: number } | null = null;
  let inline: number | null = null;
  const quotes: string[] = [];
  const closingQuotes: Record<string, string> = { "「": "」", "『": "』", "“": "”", "‘": "’", '"': '"', "'": "'" };
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trimStart();
    const marker = line.match(/^(`{3,}|~{3,})/);
    if (marker) {
      if (!fence) fence = { marker: marker[0][0], length: marker[0].length };
      else if (fence.marker === marker[0][0] && marker[0].length >= fence.length && !line.slice(marker[0].length).trim()) fence = null;
      continue;
    }
    if (fence || line.startsWith("文本附件：") || line.startsWith(">") || raw.startsWith("    ") || raw.startsWith("\t")) continue;
    for (let cursor = 0; cursor < line.length; cursor++) {
      if (inline === null) {
        const character = line[cursor];
        if (quotes.at(-1) === character) { quotes.pop(); continue; }
        const closing = closingQuotes[character];
        if (closing && (character !== "'" || !/[a-z0-9]/i.test(line[cursor - 1] ?? ""))) quotes.push(closing);
        if (quotes.length) continue;
      }
      if (line[cursor] === "`") {
        const width = line.slice(cursor).match(/^`+/)![0].length;
        if (inline === width) inline = null;
        else if (inline === null) inline = width;
        cursor += width - 1;
        continue;
      }
      if (inline !== null || line[cursor] !== "@" || /[a-z0-9_@\\]/i.test(line[cursor - 1] ?? "")) continue;
      for (const agent of agents.filter(a => members.includes(a.id))) {
        const uniqueName = agents.filter(a => members.includes(a.id) && a.name.toLowerCase() === agent.name.toLowerCase()).length === 1;
        for (const label of uniqueName ? [agent.id, agent.name] : [agent.id]) {
          const prefix = `@${label}`;
          if (line.slice(cursor, cursor + prefix.length).toLowerCase() === prefix.toLowerCase() && !/[a-z0-9_-]/i.test(line[cursor + prefix.length] ?? "") && !named.includes(agent.id)) named.push(agent.id);
        }
      }
    }
  }
  return named.length ? named : members;
}
