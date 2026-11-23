use crate::storage::Store;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

pub const MAX_BYTES: usize = 10 * 1024 * 1024;
const PREFIX: &str = "ai-gent-attachment:";

#[derive(Debug, Clone, Serialize)]
pub struct SavedAttachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: usize,
    pub path: String,
    pub is_image: bool,
}

#[derive(Debug, Clone)]
pub struct AttachmentFile {
    pub path: PathBuf,
    pub mime_type: String,
    pub is_image: bool,
}

fn directory(store: &Store) -> Result<PathBuf, String> {
    Ok(store.path.parent().ok_or("附件目录不可用")?.join("attachments"))
}

fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == '_');
    if trimmed.is_empty() {
        "attachment".to_owned()
    } else if trimmed.len() > 80 {
        trimmed.chars().take(80).collect()
    } else {
        trimmed.to_owned()
    }
}

fn extension_for_mime(mime_type: &str) -> Option<&'static str> {
    match mime_type.to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "image/bmp" => Some("bmp"),
        "application/pdf" => Some("pdf"),
        "text/plain" => Some("txt"),
        "text/markdown" => Some("md"),
        "text/csv" => Some("csv"),
        "application/json" => Some("json"),
        _ => None,
    }
}

pub fn mime_for_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        Some("md") => "text/markdown",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

fn is_image_mime(mime: &str) -> bool {
    matches!(
        mime.to_ascii_lowercase().as_str(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/webp" | "image/gif" | "image/bmp"
    )
}

pub fn save(store: &Store, name: &str, mime_type: &str, base64_data: &str) -> Result<SavedAttachment, String> {
    let payload = base64_data
        .split_once(";base64,")
        .map(|(_, data)| data)
        .unwrap_or(base64_data);
    if payload.len() > MAX_BYTES * 2 {
        return Err("附件过大，单个文件请控制在 10 MB 以内。".into());
    }
    let bytes = STANDARD.decode(payload.trim()).map_err(|_| "附件编码无效")?;
    if bytes.len() > MAX_BYTES {
        return Err("附件过大，单个文件请控制在 10 MB 以内。".into());
    }

    let root = directory(store)?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;

    let id = uuid::Uuid::new_v4().to_string();
    let mut safe_name = sanitize_name(name);
    if !safe_name.contains('.') {
        if let Some(ext) = extension_for_mime(mime_type) {
            safe_name = format!("{safe_name}.{ext}");
        }
    }
    let file_path = root.join(format!("{id}-{safe_name}"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file_path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;

    let resolved_mime = if mime_type.trim().is_empty() {
        mime_for_path(&file_path).to_owned()
    } else {
        mime_type.trim().to_owned()
    };
    let is_image = is_image_mime(&resolved_mime) || is_image_mime(mime_for_path(&file_path));

    Ok(SavedAttachment {
        id,
        name: name.to_owned(),
        mime_type: resolved_mime,
        size: bytes.len(),
        path: file_path.to_string_lossy().into_owned(),
        is_image,
    })
}

pub fn find(store: &Store, id: &str) -> Result<AttachmentFile, String> {
    let uuid = uuid::Uuid::parse_str(id).map_err(|_| "附件编号无效")?;
    let prefix = format!("{uuid}-");
    let root = directory(store)?;
    let entries = fs::read_dir(&root).map_err(|_| "附件不存在或无法读取")?;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        if name_str.starts_with(&prefix) {
            let path = entry.path();
            let mime = mime_for_path(&path).to_owned();
            return Ok(AttachmentFile {
                is_image: is_image_mime(&mime),
                mime_type: mime,
                path,
            });
        }
    }
    Err("附件不存在或无法读取".into())
}

pub fn read_data_url(store: &Store, id: &str) -> Result<String, String> {
    let file_info = find(store, id)?;
    if !file_info.is_image {
        return Err("仅支持预览图片附件".into());
    }
    let file = fs::File::open(&file_info.path).map_err(|_| "附件不存在或无法读取")?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("附件过大".into());
    }
    Ok(format!("data:{};base64,{}", file_info.mime_type, STANDARD.encode(bytes)))
}

pub fn read_base64(store: &Store, id: &str) -> Result<(String, String), String> {
    let file_info = find(store, id)?;
    let file = fs::File::open(&file_info.path).map_err(|_| "附件不存在或无法读取")?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("附件过大".into());
    }
    Ok((file_info.mime_type, STANDARD.encode(bytes)))
}

pub fn extract_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(PREFIX) {
        let after = &rest[pos + PREFIX.len()..];
        if after.len() >= 36 {
            let candidate = &after[..36];
            if uuid::Uuid::parse_str(candidate).is_ok() && !ids.iter().any(|existing| existing == candidate) {
                ids.push(candidate.to_owned());
            }
        }
        rest = after;
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_finds_and_reads_image_attachments() {
        let root = std::env::temp_dir().join(format!("agentcrew-attachments-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let store = Store { path: root.join("ai-gent.sqlite") };

        let saved = save(&store, "screenshot.png", "image/png", "iVBORw0KGgo=").unwrap();
        assert!(saved.is_image);
        assert!(PathBuf::from(&saved.path).exists());

        let found = find(&store, &saved.id).unwrap();
        assert_eq!(found.mime_type, "image/png");
        assert!(found.is_image);

        let url = read_data_url(&store, &saved.id).unwrap();
        assert_eq!(url, "data:image/png;base64,iVBORw0KGgo=");

        let ids = extract_ids(&format!("look at ![shot](ai-gent-attachment:{})", saved.id));
        assert_eq!(ids, vec![saved.id]);

        let _ = fs::remove_dir_all(&root);
    }
}
