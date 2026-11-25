export interface Skill { name: string; description: string; path: string; source: string }

export interface SkillSpan {
  start: number;
  end: number;
  name: string;
}

const LEADING_MENTIONS = /^\s*(?:@[\p{L}\p{N}_-]+\s+)*/u;
const SKILL_TOKEN = /^\/([\p{L}\p{N}_.:-]+)(?=\s|$)/u;

// Only the command prefix is interpreted; paths and slashes in prose stay text.
export function skillPrefix(text: string) {
  return text.match(/^\s*(?:@[\p{L}\p{N}_-]+\s+)*(?:\/[\p{L}\p{N}_.:-]+\s+)*\/([\p{L}\p{N}_.:-]*)$/iu);
}

export function skillCommandSpans(text: string): SkillSpan[] {
  if (!text) return [];
  const lead = text.match(LEADING_MENTIONS);
  let cursor = lead ? lead[0].length : 0;
  const spans: SkillSpan[] = [];
  while (cursor < text.length) {
    const match = text.slice(cursor).match(SKILL_TOKEN);
    if (!match) break;
    const end = cursor + 1 + match[1].length;
    spans.push({ start: cursor, end, name: match[1] });
    const ws = text.slice(end).match(/^\s+/);
    if (!ws) break;
    cursor = end + ws[0].length;
  }
  return spans;
}

export function requestedSkills(text: string) {
  return [...new Set(skillCommandSpans(text).map(span => span.name))];
}

// Deletes an entire /skill token (plus a single trailing space when present) on Backspace.
export function deleteSkillTokenAtCursor(
  text: string,
  selectionStart: number,
  selectionEnd: number,
): { text: string; cursor: number } | null {
  if (selectionStart !== selectionEnd || selectionStart <= 0) return null;
  const spans = skillCommandSpans(text);
  for (const span of spans) {
    if (selectionStart === span.end) {
      const deleteEnd = text[span.end] === " " ? span.end + 1 : span.end;
      return { text: text.slice(0, span.start) + text.slice(deleteEnd), cursor: span.start };
    }
    if (selectionStart === span.end + 1 && text[span.end] === " ") {
      return { text: text.slice(0, span.start) + text.slice(selectionStart), cursor: span.start };
    }
  }
  return null;
}
