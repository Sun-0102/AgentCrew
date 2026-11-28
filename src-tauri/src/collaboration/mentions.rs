use super::*;

pub(super) fn message_recipients_with(store: &Store, group: &str, text: &str) -> Result<Vec<String>, String> {
    let named = named_mentions(store, group, text)?;
    Ok(if named.is_empty() {
        store.group(group)?.members
    } else {
        named
    })
}
#[cfg(test)]
pub(super) fn mentions(text: &str) -> Vec<String> {
    mentions_with(
        text,
        &[
            ("@Codex".into(), "codex".into()),
            ("@DeepSeek".into(), "deepseek".into()),
        ],
    )
}
#[cfg(test)]
// Mentions in ordinary text wake members. Quoted material, code blocks and inline code do not.
pub(super) fn message_recipients(text: &str, members: &[String]) -> Vec<String> {
    let targets: Vec<(String, String)> = [("@Codex", "codex"), ("@DeepSeek", "deepseek")]
        .into_iter()
        .filter(|(_, id)| members.iter().any(|member| member == id))
        .map(|(label, id)| (label.to_owned(), id.to_owned()))
        .collect();
    let named = mentions_with(text, &targets);
    if named.is_empty() {
        members.to_vec()
    } else {
        named
    }
}

pub(super) fn mentions_with(text: &str, targets: &[(String, String)]) -> Vec<String> {
    let mut result = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut inline: Option<usize> = None;
    let mut quotes: Vec<char> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if let Some(marker @ (b'`' | b'~')) = line.as_bytes().first().copied() {
            let width = line.bytes().take_while(|byte| *byte == marker).count();
            if width >= 3 {
                match fence {
                    Some((open, length))
                        if open == marker && width >= length && line[width..].trim().is_empty() =>
                    {
                        fence = None
                    }
                    None => fence = Some((marker, width)),
                    _ => {}
                }
                continue;
            }
        }
        if fence.is_some()
            || line.starts_with("文本附件：")
            || line.starts_with('>')
            || raw.starts_with("    ")
            || raw.starts_with('\t')
        {
            continue;
        }
        let mut cursor = 0;
        while cursor < line.len() {
            let rest = &line[cursor..];
            let character = rest.chars().next().unwrap();
            if inline.is_none() {
                if quotes.last() == Some(&character) {
                    quotes.pop();
                    cursor += character.len_utf8();
                    continue;
                }
                let closing = match character {
                    '「' => Some('」'),
                    '『' => Some('』'),
                    '“' => Some('”'),
                    '‘' => Some('’'),
                    '"' => Some('"'),
                    '\'' if !line[..cursor]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_ascii_alphanumeric()) =>
                    {
                        Some('\'')
                    }
                    _ => None,
                };
                if let Some(closing) = closing {
                    quotes.push(closing);
                }
                if !quotes.is_empty() {
                    cursor += character.len_utf8();
                    continue;
                }
            }
            if rest.starts_with('`') {
                let width = rest.bytes().take_while(|byte| *byte == b'`').count();
                if inline == Some(width) {
                    inline = None;
                } else if inline.is_none() {
                    inline = Some(width);
                }
                cursor += width;
                continue;
            }
            if inline.is_none() && rest.starts_with('@') {
                let previous = line[..cursor].chars().next_back();
                if !previous
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@' || c == '\\')
                {
                    for (name, target) in targets {
                        if rest
                            .get(..name.len())
                            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
                            && !rest[name.len()..]
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                            && !result.contains(target)
                        {
                            result.push(target.clone());
                        }
                    }
                }
            }
            cursor += rest.chars().next().map(char::len_utf8).unwrap_or(1);
        }
    }
    result
}
