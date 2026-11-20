export interface Skill { name: string; description: string; path: string; source: string }

// Only the command prefix is interpreted; paths and slashes in prose stay text.
export function skillPrefix(text: string) {
  return text.match(/^\s*(?:(?:@(?:Codex|DeepSeek))\s+)*(?:\/[\p{L}\p{N}_.:-]+\s+)*\/([\p{L}\p{N}_.:-]*)$/iu);
}

export function requestedSkills(text: string) {
  const prefix = text.replace(/^\s*(?:(?:@(?:Codex|DeepSeek))\s+)*/i, "");
  const commands = prefix.match(/^(?:\/[\p{L}\p{N}_.:-]+(?:\s+|$))+/u)?.[0] ?? "";
  return [...new Set([...commands.matchAll(/\/([\p{L}\p{N}_.:-]+)/gu)].map(match => match[1]))];
}
