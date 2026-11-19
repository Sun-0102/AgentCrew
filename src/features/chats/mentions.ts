export function activeMention(text: string, cursor: number) {
  const match = text.slice(0, cursor).match(/(?:^|[\s，。！？、：；（(])@([a-z]*)$/i);
  return match ? { start: cursor - match[1].length - 1, query: match[1].toLowerCase() } : null;
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
      if (line[cursor] === "`") {
        const width = line.slice(cursor).match(/^`+/)![0].length;
        if (inline === width) inline = null;
        else if (inline === null) inline = width;
        cursor += width - 1;
        continue;
      }
      if (inline !== null || line[cursor] !== "@" || /[a-z0-9_@\\]/i.test(line[cursor - 1] ?? "")) continue;
      const match = line.slice(cursor).match(/^@(Codex|DeepSeek)(?![a-z0-9_])/i);
      if (match && !named.includes(match[1].toLowerCase())) named.push(match[1].toLowerCase());
    }
  }
  return named.length ? named : members;
}
