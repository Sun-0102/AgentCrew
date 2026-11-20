use crate::storage::Store;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

const INLINE: &str = "data:image/png;base64,";
const LIMIT: usize = 40_000_000;

fn directory(store: &Store) -> Result<PathBuf, String> {
    Ok(store.path.parent().ok_or("图片目录不可用")?.join("images"))
}

fn decode(data: &str) -> Result<Vec<u8>, String> {
    if data.len() > LIMIT {
        return Err("图片过大".into());
    }
    let bytes = STANDARD.decode(data).map_err(|_| "图片编码无效")?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("不是 PNG 图片".into());
    }
    Ok(bytes)
}

pub fn save(store: &Store, data: &str) -> Result<String, String> {
    let bytes = decode(data)?;
    let root = directory(store)?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(format!("{id}.png")))
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(format!("ai-gent-image:{id}"))
}

pub fn read(store: &Store, id: &str) -> Result<String, String> {
    let id = uuid::Uuid::parse_str(id).map_err(|_| "图片编号无效")?;
    let file = fs::File::open(directory(store)?.join(format!("{id}.png")))
        .map_err(|_| "图片不存在或无法读取")?;
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("图片文件无效".into());
    }
    Ok(format!("{INLINE}{}", STANDARD.encode(bytes)))
}

fn extract(store: &Store, text: &str) -> Result<String, String> {
    let mut rest = text;
    let mut result = String::new();
    while let Some(start) = rest.find(INLINE) {
        result.push_str(&rest[..start]);
        rest = &rest[start + INLINE.len()..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '+' | '/' | '='))
            .unwrap_or(rest.len());
        if decode(&rest[..end]).is_ok() {
            result.push_str(&save(store, &rest[..end])?);
        } else {
            result.push_str(INLINE);
            result.push_str(&rest[..end]);
        }
        rest = &rest[end..];
    }
    result.push_str(rest);
    Ok(result)
}

// Upgrade the inline images written by the first image-support build, including task archives.
pub fn recover(store: &Store) -> Result<(), String> {
    let mut connection = store.connect()?;
    let messages = {
        let mut query = connection
            .prepare("SELECT sequence,text FROM codex_messages WHERE instr(text,?1)>0")
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([INLINE], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let steps = {
        let mut query = connection
            .prepare("SELECT id,messages FROM group_task_steps WHERE instr(messages,?1)>0")
            .map_err(|e| e.to_string())?;
        let rows = query
            .query_map([INLINE], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    if messages.is_empty() && steps.is_empty() {
        return Ok(());
    }
    let backup = store.path.with_extension(format!(
        "before-image-files-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    connection
        .execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])
        .map_err(|e| e.to_string())?;
    let tx = connection.transaction().map_err(|e| e.to_string())?;
    for (sequence, text) in messages {
        tx.execute(
            "UPDATE codex_messages SET text=?2 WHERE sequence=?1",
            rusqlite::params![sequence, extract(store, &text)?],
        )
        .map_err(|e| e.to_string())?;
    }
    for (id, text) in steps {
        let mut messages: Vec<Value> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        for message in &mut messages {
            if let Some(text) = message["text"].as_str() {
                if text.contains(INLINE) {
                    message["text"] = Value::String(extract(store, text)?);
                }
            }
        }
        tx.execute(
            "UPDATE group_task_steps SET messages=?2 WHERE id=?1",
            rusqlite::params![
                id,
                serde_json::to_string(&messages).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrates_old_images_and_loads_only_named_png_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.sqlite")).unwrap();
        let project = store.add_project(dir.path().to_str().unwrap()).unwrap();
        let group = store
            .create_group_in(
                "images",
                vec!["codex".into()],
                None,
                &dir.path().join("groups"),
            )
            .unwrap();
        let data = STANDARD.encode(b"\x89PNG\r\n\x1a\nexample");
        let text = format!("before\n![dog]({INLINE}{data})\nafter");
        store.connect().unwrap().execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,'image','assistant',?2)",rusqlite::params![project.id,text]).unwrap();
        let archive =
            serde_json::json!([{"id":"image","role":"assistant","text":text}]).to_string();
        let connection = store.connect().unwrap();
        connection.execute("INSERT INTO group_tasks(id,conversation_id,objective,status,current_agent,codex_model,deepseek_model,pending_prompt) VALUES('task',?1,'image','completed','codex','model','','')",[group.id]).unwrap();
        connection.execute("INSERT INTO group_task_steps(task_id,ordinal,agent,source_agent,instruction,status,sequence_start,before_snapshot,messages) VALUES('task',1,'codex','user','','completed',0,'{}',?1)",[archive]).unwrap();
        recover(&store).unwrap();
        let message: String = store
            .connect()
            .unwrap()
            .query_row("SELECT text FROM codex_messages WHERE item_id='image'", [], |r| r.get(0))
            .unwrap();
        assert!(!message.contains(INLINE));
        let id = message
            .split("ai-gent-image:")
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap();
        assert_eq!(read(&store, id).unwrap(), format!("{INLINE}{data}"));
        assert!(message.starts_with("before\n![dog](ai-gent-image:"));
        assert!(message.ends_with(")\nafter"));
        let archived: String = store
            .connect()
            .unwrap()
            .query_row("SELECT messages FROM group_task_steps", [], |r| r.get(0))
            .unwrap();
        assert!(!archived.contains(INLINE));
        assert!(archived.contains("ai-gent-image:"));
        let count = fs::read_dir(directory(&store).unwrap()).unwrap().count();
        recover(&store).unwrap();
        assert_eq!(
            fs::read_dir(directory(&store).unwrap()).unwrap().count(),
            count
        );
        assert!(read(&store, "../../auth.json").is_err());
        assert!(save(&store, "invalid").is_err());
        assert!(save(&store, &STANDARD.encode(b"not a png")).is_err());
        assert_eq!(
            extract(&store, "example data:image/png;base64,invalid!").unwrap(),
            "example data:image/png;base64,invalid!"
        );
    }
}
