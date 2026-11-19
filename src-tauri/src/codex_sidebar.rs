use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn sync(database: &Path, thread_id: &str, root: &str) -> Result<(), String> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .ok_or("无法定位 Codex 数据目录")?;
    sync_in(&home, database, thread_id, root)
}

fn sync_in(home: &Path, database: &Path, thread_id: &str, root: &str) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(thread_id).map_err(|_| "会话 ID 不是 UUID，索引同步已停止")?;
    let mut files: Vec<_> = std::fs::read_dir(home)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "sqlite")
                && path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.strip_prefix("state_")
                            .is_some_and(|v| v.parse::<u32>().is_ok())
                    })
        })
        .collect();
    files.sort_by_key(|path| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("state_"))
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0)
    });
    for file in files.into_iter().rev() {
        let mut connection = Connection::open_with_flags(&file, OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(|e| e.to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let row = tx.query_row("SELECT source,originator,cwd,updated_at,updated_at_ms,recency_at,recency_at_ms FROM threads WHERE id=?1",[thread_id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,String>(2)?,row.get::<_,i64>(3)?,row.get::<_,Option<i64>>(4)?,row.get::<_,Option<i64>>(5)?,row.get::<_,Option<i64>>(6)?))).optional().map_err(|e|format!("Codex 侧栏索引结构不兼容：{e}"))?;
        let Some((source, originator, cwd, updated, updated_ms, recency, recency_ms)) = row else {
            continue;
        };
        if originator.as_deref() != Some("ai_gent") {
            return Err("会话不属于 AI-gent，未修改官方索引。".into());
        }
        if std::fs::canonicalize(&cwd).map_err(|e| e.to_string())?
            != std::fs::canonicalize(root).map_err(|e| e.to_string())?
        {
            return Err("Codex 索引中的会话目录不匹配".into());
        }
        if source == "cli" {
            return Ok(());
        }
        if source != "vscode" {
            return Err(format!("暂不支持来源 {source} 的侧栏兼容同步"));
        }
        let backup_dir = database
            .parent()
            .ok_or("应用数据目录不存在")?
            .join("sidebar-index-backups");
        std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
        let backup = backup_dir.join(format!("{id}.json"));
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&backup)
        {
            Ok(mut output) => {
                let value = json!({"database":file,"thread_id":thread_id,"source":source,"originator":originator,"cwd":cwd,"updated_at":updated,"updated_at_ms":updated_ms,"recency_at":recency,"recency_at_ms":recency_ms});
                output
                    .write_all(value.to_string().as_bytes())
                    .and_then(|_| output.sync_all())
                    .map_err(|e| format!("索引备份失败：{e}"))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let saved: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&backup).map_err(|e| e.to_string())?)
                        .map_err(|e| format!("原索引备份不完整：{e}"))?;
                if saved["thread_id"] != thread_id
                    || saved["database"] != json!(file)
                    || saved["source"] != "vscode"
                {
                    return Err("原索引备份不匹配，停止同步。".into());
                }
            }
            Err(e) => return Err(format!("无法备份侧栏索引：{e}")),
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as i64;
        // compat: app-server hardcodes vscode, while Desktop 26.1111 catalogs cli only. Keep the true originator and execution permissions.
        tx.execute("UPDATE threads SET source='cli',updated_at=?2,updated_at_ms=?3,recency_at=?2,recency_at_ms=?3 WHERE id=?1 AND source='vscode' AND originator='ai_gent'",params![thread_id,now/1000,now]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(());
    }
    Err("Codex 索引尚未写入本次会话，请稍后重试连接。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_owned_threads_are_indexed_and_original_metadata_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
        db.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,source TEXT,originator TEXT,cwd TEXT,updated_at INTEGER,updated_at_ms INTEGER,recency_at INTEGER,recency_at_ms INTEGER);").unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let root = dir.path().to_str().unwrap();
        db.execute(
            "INSERT INTO threads VALUES(?1,'vscode','other-client',?2,1,1000,1,1000)",
            params![id, root],
        )
        .unwrap();
        assert!(sync_in(dir.path(), &dir.path().join("app.sqlite"), &id, root).is_err());
        db.execute("UPDATE threads SET originator='ai_gent'", [])
            .unwrap();
        sync_in(dir.path(), &dir.path().join("app.sqlite"), &id, root).unwrap();
        assert_eq!(
            db.query_row("SELECT source FROM threads", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "cli"
        );
        let backup = dir
            .path()
            .join("sidebar-index-backups")
            .join(format!("{id}.json"));
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&backup).unwrap()).unwrap();
        assert_eq!(value["source"], "vscode");
        assert_eq!(value["updated_at"], 1);
        sync_in(dir.path(), &dir.path().join("app.sqlite"), &id, root).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(backup).unwrap()).unwrap(),
            value
        );
    }
}
