use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const MAX_IDLE_CONNECTIONS_PER_DB: usize = 8;

type PoolSlot = Arc<Mutex<Vec<Connection>>>;

fn global_pools() -> &'static Mutex<HashMap<PathBuf, PoolSlot>> {
    static POOLS: OnceLock<Mutex<HashMap<PathBuf, PoolSlot>>> = OnceLock::new();
    POOLS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pool_for_path(path: &Path) -> PoolSlot {
    let mut map = global_pools().lock().unwrap_or_else(|e| e.into_inner());
    map.entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(Vec::new())))
        .clone()
}

fn clear_pool_for_path(path: &Path) {
    let slot = {
        let map = global_pools().lock().unwrap_or_else(|e| e.into_inner());
        map.get(path).cloned()
    };
    if let Some(slot) = slot {
        slot.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

pub(crate) struct PooledConnection {
    conn: Option<Connection>,
    pool: PoolSlot,
}

impl Deref for PooledConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.conn.as_ref().expect("pooled connection already taken")
    }
}

impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn.as_mut().expect("pooled connection already taken")
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            if !conn.is_autocommit() {
                return;
            }
            let mut idle = self.pool.lock().unwrap_or_else(|e| e.into_inner());
            if idle.len() < MAX_IDLE_CONNECTIONS_PER_DB {
                idle.push(conn);
            }
        }
    }
}

pub struct Store {
    pub path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub primary_root: String,
    pub created_at: String,
}

pub(crate) const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/001_projects.sql"),
    include_str!("../../migrations/002_codex.sql"),
    include_str!("../../migrations/003_conversations.sql"),
    include_str!("../../migrations/004_groups.sql"),
    include_str!("../../migrations/005_deepseek.sql"),
    include_str!("../../migrations/006_deepseek_groups.sql"),
    include_str!("../../migrations/007_group_tasks.sql"),
    include_str!("../../migrations/008_group_inputs.sql"),
    include_str!("../../migrations/009_reasoning.sql"),
    include_str!("../../migrations/010_speed.sql"),
    include_str!("../../migrations/011_task_results.sql"),
    include_str!("../../migrations/012_parallel_members.sql"),
    include_str!("../../migrations/013_group_context.sql"),
    include_str!("../../migrations/014_group_sessions.sql"),
    include_str!("../../migrations/015_agent_registry.sql"),
    include_str!("../../migrations/016_generic_pairing_gemini.sql"),
    include_str!("../../migrations/017_private_sessions.sql"),
    include_str!("../../migrations/018_usage_records.sql"),
    include_str!("../../migrations/019_claude_code.sql"),
    include_str!("../../migrations/020_project_removal.sql"),
    include_str!("../../migrations/021_project_sessions.sql"),
    include_str!("../../migrations/022_single_chat_queue.sql"),
    include_str!("../../migrations/023_parallel_group_tasks.sql"),
    include_str!("../../migrations/024_scheduled_tasks.sql"),
    include_str!("../../migrations/025_scheduled_task_sessions.sql"),
    include_str!("../../migrations/026_model_defaults.sql"),
    include_str!("../../migrations/027_knowledge_wiki.sql"),
    include_str!("../../migrations/028_wiki_private_default.sql"),
];

pub(crate) const CURRENT_SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Strips Windows verbatim path prefixes (`\\?\C:\...` -> `C:\...`, `\\?\UNC\host\share` -> `\\host\share`)
/// so paths persisted in SQLite, shown in the UI, and passed to external tools stay human-readable and compatible.
pub(crate) fn normalize_path_str(raw: &str) -> String {
    if let Some(unc) = raw
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| raw.strip_prefix(r"\\?\unc\"))
    {
        return format!(r"\\{unc}");
    }
    if let Some(rest) = raw.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return rest.to_owned();
        }
    }
    raw.to_owned()
}

pub(crate) fn normalize_path_buf(path: PathBuf) -> PathBuf {
    match path.to_str() {
        Some(s) => PathBuf::from(normalize_path_str(s)),
        None => path,
    }
}

pub(crate) fn canonicalize_path(path: &Path) -> std::io::Result<PathBuf> {
    path.canonicalize().map(normalize_path_buf)
}

fn cleanup_windows_verbatim_paths(tx: &rusqlite::Transaction<'_>) -> Result<(), String> {
    // `projects.primary_root` has a UNIQUE constraint; use UPDATE OR IGNORE in case a clean row already exists.
    let project_rows: Vec<(String, String)> = {
        let mut stmt = tx
            .prepare(r"SELECT id, primary_root FROM projects WHERE substr(primary_root, 1, 4) = '\\?\'")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    for (id, raw) in project_rows {
        let clean = normalize_path_str(&raw);
        if clean != raw {
            tx.execute(
                "UPDATE OR IGNORE projects SET primary_root = ?1 WHERE id = ?2",
                params![clean, id],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    const TABLE_COLUMNS: &[(&str, &str)] = &[
        ("conversations", "workspace_path"),
        ("deepseek_sessions", "workspace_path"),
        ("gemini_sessions", "workspace_path"),
        ("claude_sessions", "workspace_path"),
        ("agent_sessions", "workspace_path"),
        ("group_tasks", "workspace_path"),
        ("wiki_sources", "root_path"),
    ];
    for (table, column) in TABLE_COLUMNS {
        let rows: Vec<(i64, String)> = {
            let sql = format!(
                r"SELECT rowid, {column} FROM {table} WHERE {column} IS NOT NULL AND substr({column}, 1, 4) = '\\?\'"
            );
            let mut stmt = tx.prepare(&sql).map_err(|e| e.to_string())?;
            let mapped = stmt
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?;
            mapped.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        };
        if rows.is_empty() {
            continue;
        }
        let update_sql = format!("UPDATE {table} SET {column} = ?1 WHERE rowid = ?2");
        for (rowid, raw) in rows {
            let clean = normalize_path_str(&raw);
            if clean != raw {
                tx.execute(&update_sql, params![clean, rowid])
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

impl Store {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        clear_pool_for_path(&path);
        let store = Self { path };
        let mut connection = store.connect()?;
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
            .map_err(|e| e.to_string())?;
        let previous: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if (1..CURRENT_SCHEMA_VERSION).contains(&previous) {
            let backup = store
                .path
                .with_extension(format!("v{previous}-{}.sqlite", uuid::Uuid::new_v4()));
            connection
                .execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])
                .map_err(|e| format!("迁移前备份失败：{e}"))?;
        }
        let transaction = connection.transaction().map_err(|e| e.to_string())?;
        let version: u32 = transaction
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if version > CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "数据库版本 {version} 高于当前软件支持的版本，请使用匹配版本。"
            ));
        }
        for migration in MIGRATIONS.iter().skip(version as usize) {
            transaction
                .execute_batch(migration)
                .map_err(|e| e.to_string())?;
        }
        cleanup_windows_verbatim_paths(&transaction)?;
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(store)
    }

    pub(crate) fn connect(&self) -> Result<PooledConnection, String> {
        let pool = pool_for_path(&self.path);
        let cached = pool.lock().unwrap_or_else(|e| e.into_inner()).pop();
        if let Some(conn) = cached {
            return Ok(PooledConnection {
                conn: Some(conn),
                pool,
            });
        }
        let connection = Connection::open(&self.path).map_err(|e| e.to_string())?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|e| e.to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| e.to_string())?;
        let _ = connection.pragma_update(None, "synchronous", "NORMAL");
        Ok(PooledConnection {
            conn: Some(connection),
            pool,
        })
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, String> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, primary_root, created_at FROM projects WHERE removed_at IS NULL ORDER BY created_at, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    primary_root: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn add_project(&self, input: &str) -> Result<Project, String> {
        self.add_project_with_name(input, None)
    }

    pub fn add_project_with_name(&self, input: &str, custom_name: Option<&str>) -> Result<Project, String> {
        let canonical = canonicalize_path(Path::new(input))
            .map_err(|e| format!("无法访问项目目录：{e}"))?;
        if !canonical.is_dir() {
            return Err("项目路径必须是目录。".into());
        }
        std::fs::read_dir(&canonical).map_err(|e| format!("项目目录不可读：{e}"))?;
        let path = canonical
            .to_str()
            .ok_or("项目目录不是有效的 Unicode 路径。")?;
        let default_name = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path);
        let trimmed_custom = custom_name.map(str::trim).filter(|s| !s.is_empty());
        let name = trimmed_custom.unwrap_or(default_name);
        let connection = self.connect()?;
        connection
            .execute(
                "INSERT INTO projects (id, name, primary_root) VALUES (?1, ?2, ?3) ON CONFLICT(primary_root) DO UPDATE SET removed_at=NULL, name=CASE WHEN ?4 IS NOT NULL THEN ?4 ELSE name END",
                params![uuid::Uuid::new_v4().to_string(), name, path, trimmed_custom],
            )
            .map_err(|e| e.to_string())?;
        connection.execute("INSERT INTO conversations(id,project_id,workspace_path) SELECT id,id,primary_root FROM projects WHERE primary_root=?1 ON CONFLICT(id) DO NOTHING",[path]).map_err(|e|e.to_string())?;
        connection
            .query_row(
                "SELECT id, name, primary_root, created_at FROM projects WHERE primary_root = ?1",
                [path],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        primary_root: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .map_err(|e| e.to_string())
    }

    pub fn rename_project(&self, id: &str, custom_name: Option<&str>) -> Result<Project, String> {
        let connection = self.connect()?;
        let primary_root: String = connection
            .query_row(
                "SELECT primary_root FROM projects WHERE id=?1 AND removed_at IS NULL",
                [id],
                |row| row.get(0),
            )
            .map_err(|_| "项目不存在或已移除。".to_owned())?;
        let default_name = Path::new(&primary_root)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&primary_root);
        let name = custom_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(default_name);
        connection
            .execute(
                "UPDATE projects SET name=?1 WHERE id=?2 AND removed_at IS NULL",
                params![name, id],
            )
            .map_err(|e| e.to_string())?;
        connection
            .query_row(
                "SELECT id, name, primary_root, created_at FROM projects WHERE id=?1",
                [id],
                |row| {
                    Ok(Project {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        primary_root: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .map_err(|e| e.to_string())
    }

    pub fn remove_project(&self, id: &str) -> Result<(), String> {
        let changed = self.connect()?.execute(
            "UPDATE projects SET removed_at=strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id=?1 AND removed_at IS NULL",
            [id],
        ).map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("项目不存在或已移除。".into());
        }
        Ok(())
    }

    pub fn conversation_id(&self, project_id: Option<&str>) -> Result<String, String> {
        if let Some(id) = project_id {
            self.conversation_root(id)?;
            return Ok(id.to_owned());
        }
        self.personal_conversation_id(&self.chat_workspace_parent()?)
    }

    pub(crate) fn chat_workspace_parent(&self) -> Result<PathBuf, String> {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .ok_or("无法定位用户主目录")?;
        let app_name = self
            .path
            .parent()
            .and_then(Path::file_name)
            .ok_or("本地数据目录不存在")?;
        // shortcut: Codex Desktop 0.169 discovers projectless chats under ~/Documents/Codex; recheck on client layout changes.
        let parent = PathBuf::from(home)
            .join("Documents")
            .join("Codex")
            .join("AgentCrew")
            .join(app_name);
        Ok(parent)
    }

    pub(crate) fn personal_conversation_id(&self, parent: &Path) -> Result<String, String> {
        use rusqlite::OptionalExtension;
        let id = "codex-personal";
        std::fs::create_dir_all(parent).map_err(|e| format!("无法建立私聊目录：{e}"))?;
        let parent = canonicalize_path(parent).map_err(|e| e.to_string())?;
        let directory = parent.join(id);
        let mut connection = self.connect()?;
        let tx = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT workspace_path FROM conversations WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(old) = existing {
            let old = PathBuf::from(old);
            if old != directory {
                let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM codex_sessions WHERE conversation_id=?1 AND status IN ('starting','running','waiting_approval','cancel_requested','stopping'))",[id],|row|row.get(0)).map_err(|e|e.to_string())?;
                if active {
                    return Err("私聊正在执行，请完成或取消后再迁移目录。".into());
                }
                let expected = self
                    .path
                    .parent()
                    .ok_or("本地数据目录不存在")?
                    .join("workspaces")
                    .join(id)
                    .canonicalize()
                    .map_err(|e| format!("无法核对旧私聊目录：{e}"))?;
                if old.canonicalize().map_err(|e| e.to_string())? != expected {
                    return Err("旧私聊目录不是应用管理的目录，迁移已停止。".into());
                }
                if directory.exists() {
                    return Err("新的私聊目录已存在，迁移已停止，原记录和文件均保留。".into());
                }
                let staging = parent.join(format!(".codex-personal-{}", uuid::Uuid::new_v4()));
                copy_workspace(&old, &staging)
                    .map_err(|e| format!("复制私聊文件失败，旧目录已保留：{e}"))?;
                if !staging.starts_with(&parent) || !directory.starts_with(&parent) {
                    return Err("私聊迁移目标超出预期目录".into());
                }
                std::fs::rename(&staging, &directory)
                    .map_err(|e| format!("私聊目录迁移失败，旧目录已保留：{e}"))?;
            }
        } else {
            std::fs::create_dir(&directory)
                .or_else(|e| {
                    if e.kind() == std::io::ErrorKind::AlreadyExists && directory.is_dir() {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        let root = canonicalize_path(&directory).map_err(|e| e.to_string())?;
        let root = root.to_str().ok_or("私聊工作目录不是有效 Unicode 路径")?;
        tx.execute("INSERT INTO conversations(id,project_id,workspace_path) VALUES(?1,NULL,?2) ON CONFLICT(id) DO UPDATE SET workspace_path=excluded.workspace_path",params![id,root]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id.to_owned())
    }

    pub fn conversation_root(&self, id: &str) -> Result<String, String> {
        self.connect()?
            .query_row(
                "SELECT workspace_path FROM conversations WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|_| "会话不存在，请选择有效项目或打开 Codex 私聊。".into())
    }
}

fn copy_workspace(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "私聊目录包含链接，请先人工迁移该链接",
        ));
    }
    if metadata.is_dir() {
        std::fs::create_dir(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_workspace(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_persist_and_duplicate_paths_keep_the_same_id() {
        let temp = tempfile::tempdir().unwrap();
        let project_dir = temp.path().join("中文 项目");
        std::fs::create_dir(&project_dir).unwrap();
        let database = temp.path().join("test.sqlite");
        let store = Store::open(database.clone()).unwrap();
        let first = store.add_project(project_dir.to_str().unwrap()).unwrap();
        let duplicate = store
            .add_project(project_dir.join(".").to_str().unwrap())
            .unwrap();
        assert_eq!(first.id, duplicate.id);
        let other_dir = temp.path().join("另一个项目");
        std::fs::create_dir(&other_dir).unwrap();
        let other = store.add_project(other_dir.to_str().unwrap()).unwrap();
        assert_ne!(first.id, other.id);
        drop(store);
        let projects = Store::open(database).unwrap().list_projects().unwrap();
        assert_eq!(projects.len(), 2);
        assert!(projects
            .iter()
            .any(|project| project.id == first.id && project.name == "中文 项目"));
        assert!(projects
            .iter()
            .any(|project| project.id == other.id && project.name == "另一个项目"));
        assert!(projects[0].created_at.ends_with('Z'));
    }

    #[test]
    fn files_and_missing_directories_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file.txt");
        std::fs::write(&file, "test").unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        assert!(store.add_project(file.to_str().unwrap()).is_err());
        assert!(store
            .add_project(temp.path().join("missing").to_str().unwrap())
            .is_err());
        assert!(store.list_projects().unwrap().is_empty());
    }

    #[test]
    fn project_database_upgrade_keeps_a_readable_backup() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("upgrade.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../../migrations/001_projects.sql"))
            .unwrap();
        connection.execute("INSERT INTO projects(id,name,primary_root) VALUES('old','existing','existing-root')",[]).unwrap();
        drop(connection);
        let store = Store::open(path).unwrap();
        assert_eq!(store.list_projects().unwrap()[0].id, "old");
        let backup = std::fs::read_dir(temp.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|p| p.file_name().unwrap().to_string_lossy().contains("v1-"))
            .unwrap();
        let backup = Connection::open(backup).unwrap();
        assert_eq!(
            backup
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            backup
                .query_row("SELECT id FROM projects", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "old"
        );
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            CURRENT_SCHEMA_VERSION
        );
    }

    #[test]
    fn personal_conversation_persists_without_creating_a_project() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("personal.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let id = store
            .personal_conversation_id(&temp.path().join("Documents/Codex/AgentCrew"))
            .unwrap();
        let root = store.conversation_root(&id).unwrap();
        assert!(Path::new(&root).is_dir());
        assert!(store.list_projects().unwrap().is_empty());
        let project_dir = temp.path().join("project");
        std::fs::create_dir(&project_dir).unwrap();
        let project = store.add_project(project_dir.to_str().unwrap()).unwrap();
        assert_ne!(id, project.id);
        assert_ne!(root, store.conversation_root(&project.id).unwrap());
        store.connect().unwrap().execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES(?1,'personal-item','user','private')",[&id]).unwrap();
        let store = Store::open(path).unwrap();
        assert_eq!(
            store
                .personal_conversation_id(&temp.path().join("Documents/Codex/AgentCrew"))
                .unwrap(),
            id
        );
        assert_eq!(store.conversation_root(&id).unwrap(), root);
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM codex_messages WHERE conversation_id=?1",
                    [&project.id],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
        assert!(store.conversation_id(Some("missing-project")).is_err());
    }

    #[test]
    fn personal_workspace_migration_keeps_files_history_and_original_copy() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        let id = store
            .personal_conversation_id(&temp.path().join("workspaces"))
            .unwrap();
        let old = PathBuf::from(store.conversation_root(&id).unwrap());
        std::fs::create_dir(old.join("nested")).unwrap();
        std::fs::write(old.join("nested/artifact.txt"), "keep me").unwrap();
        store.connect().unwrap().execute("INSERT INTO codex_sessions(conversation_id,thread_id,model,runtime_version,provider,status) VALUES(?1,'same-thread','model','version','provider','running')",[&id]).unwrap();
        let official = temp.path().join("Documents/Codex/AgentCrew");
        assert!(store.personal_conversation_id(&official).is_err());
        assert_eq!(store.conversation_root(&id).unwrap(), old.to_str().unwrap());
        store
            .connect()
            .unwrap()
            .execute("UPDATE codex_sessions SET status='completed'", [])
            .unwrap();
        store.personal_conversation_id(&official).unwrap();
        let new = PathBuf::from(store.conversation_root(&id).unwrap());
        assert!(new.starts_with(canonicalize_path(&official).unwrap()));
        assert_eq!(
            std::fs::read_to_string(new.join("nested/artifact.txt")).unwrap(),
            "keep me"
        );
        assert_eq!(
            std::fs::read_to_string(old.join("nested/artifact.txt")).unwrap(),
            "keep me"
        );
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row(
                    "SELECT thread_id FROM codex_sessions WHERE conversation_id=?1",
                    [&id],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "same-thread"
        );
        assert_eq!(store.personal_conversation_id(&official).unwrap(), id);
    }

    #[test]
    fn personal_workspace_migration_does_not_overwrite_a_conflicting_directory() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path().join("test.sqlite")).unwrap();
        let id = store
            .personal_conversation_id(&temp.path().join("workspaces"))
            .unwrap();
        let old = store.conversation_root(&id).unwrap();
        let official = temp.path().join("Documents/Codex/AgentCrew");
        std::fs::create_dir_all(official.join("codex-personal")).unwrap();
        std::fs::write(official.join("codex-personal/existing.txt"), "existing").unwrap();
        assert!(store.personal_conversation_id(&official).is_err());
        assert_eq!(store.conversation_root(&id).unwrap(), old);
        assert_eq!(
            std::fs::read_to_string(official.join("codex-personal/existing.txt")).unwrap(),
            "existing"
        );
    }

    #[test]
    fn version_two_upgrade_preserves_project_sessions_and_messages() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("old.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../../migrations/001_projects.sql"))
            .unwrap();
        connection
            .execute_batch(include_str!("../../migrations/002_codex.sql"))
            .unwrap();
        connection
            .execute(
                "INSERT INTO projects(id,name,primary_root) VALUES('project','existing','root')",
                [],
            )
            .unwrap();
        connection.execute("INSERT INTO codex_sessions(project_id,thread_id,model,runtime_version,provider,status) VALUES('project','thread','model','version','provider','completed')",[]).unwrap();
        connection.execute("INSERT INTO codex_messages(project_id,item_id,role,text) VALUES('project','message','assistant','saved reply')",[]).unwrap();
        drop(connection);
        let store = Store::open(path).unwrap();
        assert_eq!(store.conversation_id(Some("project")).unwrap(), "project");
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row(
                    "SELECT thread_id FROM codex_sessions WHERE conversation_id='project'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "thread"
        );
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row(
                    "SELECT text FROM codex_messages WHERE conversation_id='project'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "saved reply"
        );
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                    row.get::<_, u32>(0)
                })
                .unwrap(),
            0
        );
        assert!(std::fs::read_dir(temp.path()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("v2-")));
    }

    #[test]
    fn version_four_upgrade_backs_up_and_preserves_codex_history() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("old.sqlite");
        let connection = Connection::open(&path).unwrap();
        for migration in [
            include_str!("../../migrations/001_projects.sql"),
            include_str!("../../migrations/002_codex.sql"),
            include_str!("../../migrations/003_conversations.sql"),
            include_str!("../../migrations/004_groups.sql"),
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.execute("INSERT INTO conversations(id,project_id,workspace_path) VALUES('codex-personal',NULL,'workspace')",[]).unwrap();
        connection.execute("INSERT INTO codex_sessions(conversation_id,thread_id,model,runtime_version,provider) VALUES('codex-personal','keep-thread','model','version','provider')",[]).unwrap();
        connection.execute("INSERT INTO codex_messages(conversation_id,item_id,role,text) VALUES('codex-personal','keep-message','assistant','keep history')",[]).unwrap();
        drop(connection);
        let store = Store::open(path).unwrap();
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row(
                    "SELECT text FROM codex_messages WHERE item_id='keep-message'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "keep history"
        );
        assert_eq!(
            store
                .connect()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM deepseek_sessions", [], |r| r
                    .get::<_, u32>(0))
                .unwrap(),
            0
        );
        let backup = std::fs::read_dir(temp.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.file_name().unwrap().to_string_lossy().contains("v4-"))
            .unwrap();
        assert_eq!(
            Connection::open(backup)
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            4
        );
    }

    #[test]
    fn newer_database_schema_is_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("newer.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 99;")
            .unwrap();
        assert!(Store::open(path).is_err());
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
                .unwrap(),
            99
        );
    }

    #[test]
    fn connection_pool_reuses_idle_connections_across_store_handles() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("pooled.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let slot = pool_for_path(&path);
        assert_eq!(slot.lock().unwrap().len(), 1);
        {
            let conn = store.connect().unwrap();
            assert_eq!(slot.lock().unwrap().len(), 0);
            let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
            assert_eq!(fk, 1);
        }
        assert_eq!(slot.lock().unwrap().len(), 1);
        let other_handle = Store { path: path.clone() };
        {
            let _conn = other_handle.connect().unwrap();
            assert_eq!(slot.lock().unwrap().len(), 0);
        }
        assert_eq!(slot.lock().unwrap().len(), 1);
    }

    #[test]
    fn normalize_path_str_strips_windows_verbatim_prefixes() {
        assert_eq!(
            normalize_path_str(r"\\?\C:\Users\alice\repo"),
            r"C:\Users\alice\repo"
        );
        assert_eq!(
            normalize_path_str(r"\\?\d:\work\proj"),
            r"d:\work\proj"
        );
        assert_eq!(
            normalize_path_str(r"\\?\UNC\server\share\repo"),
            r"\\server\share\repo"
        );
        assert_eq!(
            normalize_path_str(r"\\?\unc\server\share\repo"),
            r"\\server\share\repo"
        );
        // Non-drive / device verbatim paths and normal paths must stay intact.
        assert_eq!(
            normalize_path_str(r"\\?\Volume{1234}\foo"),
            r"\\?\Volume{1234}\foo"
        );
        assert_eq!(normalize_path_str("/Users/alice/repo"), "/Users/alice/repo");
        assert_eq!(
            normalize_path_str(r"C:\Users\alice\repo"),
            r"C:\Users\alice\repo"
        );
    }

    #[test]
    fn store_open_cleans_existing_windows_verbatim_paths() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("win-paths.sqlite");
        {
            let store = Store::open(db_path.clone()).unwrap();
            let conn = store.connect().unwrap();
            conn.execute(
                r"INSERT INTO projects(id, name, primary_root) VALUES('p1', 'Proj', '\\?\C:\Users\alice\proj')",
                [],
            )
            .unwrap();
            conn.execute(
                r"INSERT INTO conversations(id, project_id, workspace_path) VALUES('c1', 'p1', '\\?\C:\Users\alice\proj')",
                [],
            )
            .unwrap();
            conn.execute(
                r"INSERT INTO wiki_sources(id, name, source_kind, tier, root_path, content_subdir, inbox_subdir, index_state) VALUES('w1', 'Wiki', 'local_folder', 'core_wiki', '\\?\UNC\nas\docs\wiki', '', '00-inbox', 'idle')",
                [],
            )
            .unwrap();
        }

        let reopened = Store::open(db_path).unwrap();
        let projects = reopened.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].primary_root, r"C:\Users\alice\proj");
        assert_eq!(
            reopened.conversation_root("c1").unwrap(),
            r"C:\Users\alice\proj"
        );
        let wiki_root: String = reopened
            .connect()
            .unwrap()
            .query_row("SELECT root_path FROM wiki_sources WHERE id = 'w1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(wiki_root, r"\\nas\docs\wiki");
    }
}
