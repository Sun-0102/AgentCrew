use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};

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

impl Store {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let store = Self { path };
        let mut connection = store.connect()?;
        let transaction = connection.transaction().map_err(|e| e.to_string())?;
        let version: u32 = transaction
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        match version {
            0 => transaction
                .execute_batch(include_str!("../../migrations/001_projects.sql"))
                .map_err(|e| e.to_string())?,
            1 => (),
            _ => {
                return Err(format!(
                    "数据库版本 {version} 高于当前软件支持的版本，请使用匹配版本。"
                ))
            }
        }
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(store)
    }

    fn connect(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.path).map_err(|e| e.to_string())?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        Ok(connection)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, String> {
        let connection = self.connect()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, primary_root, created_at FROM projects ORDER BY created_at, id",
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
        let canonical = Path::new(input)
            .canonicalize()
            .map_err(|e| format!("无法访问项目目录：{e}"))?;
        if !canonical.is_dir() {
            return Err("项目路径必须是目录。".into());
        }
        std::fs::read_dir(&canonical).map_err(|e| format!("项目目录不可读：{e}"))?;
        let path = canonical
            .to_str()
            .ok_or("项目目录不是有效的 Unicode 路径。")?;
        let name = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path);
        let connection = self.connect()?;
        connection
            .execute(
                "INSERT INTO projects (id, name, primary_root) VALUES (?1, ?2, ?3) ON CONFLICT(primary_root) DO NOTHING",
                params![uuid::Uuid::new_v4().to_string(), name, path],
            )
            .map_err(|e| e.to_string())?;
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
}
