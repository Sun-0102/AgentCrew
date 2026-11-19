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
