use crate::storage::Store;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use std::{collections::{BTreeMap, HashSet}, path::{Path, PathBuf}};

#[derive(Deserialize, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: String,
    #[serde(default)]
    pub native: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default)]
    pub source: String,
}

fn scalar(value: &str) -> String {
    let value = value.trim();
    if value.starts_with(['\'', '"']) {
        let quote = value.chars().next().unwrap();
        return value[1..].split(quote).next().unwrap_or_default().to_owned();
    }
    value.split(" #").next().unwrap_or_default().trim().to_owned()
}

fn collect(root: &Path, depth: usize, seen: &mut HashSet<PathBuf>, skills: &mut BTreeMap<String, Skill>) -> Result<(), String> {
    if !root.exists() || depth > 4 { return Ok(()); }
    let canonical = root.canonicalize().map_err(|e| format!("无法读取技能目录 {}：{e}", root.display()))?;
    if !seen.insert(canonical) { return Ok(()); }
    let file = root.join("SKILL.md");
    if file.is_file() {
        if file.metadata().map_err(|e| e.to_string())?.len() > 1_000_000 { return Ok(()); }
        let text = std::fs::read_to_string(&file).map_err(|e| format!("无法读取技能 {}：{e}", file.display()))?;
        let mut name = root.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let mut description = String::new();
        let mut lines = text.trim_start_matches('\u{feff}').lines();
        if lines.next() == Some("---") {
            let mut description_block = false;
            for line in lines.take_while(|line| line.trim() != "---") {
                if description_block && line.starts_with([' ', '\t']) {
                    if !description.is_empty() { description.push(' '); }
                    description.push_str(line.trim());
                    continue;
                }
                description_block = false;
                if let Some(value) = line.strip_prefix("name:") { name = scalar(value); }
                if let Some(value) = line.strip_prefix("description:") {
                    description_block = matches!(value.trim(), ">" | "|" | ">-" | "|-");
                    if !description_block { description = scalar(value); }
                }
                if line.strip_prefix("user-invocable:").is_some_and(|v| scalar(v).eq_ignore_ascii_case("false")) { return Ok(()); }
            }
        }
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')) {
            skills.entry(name.clone()).or_insert(Skill { name, description, path: file.to_string_lossy().into_owned(), native: false, content: None, source: "本地技能".into() });
        }
        return Ok(());
    }
    let mut entries = std::fs::read_dir(root).map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.path().is_dir() { collect(&entry.path(), depth + 1, seen, skills)?; }
    }
    Ok(())
}

pub fn list(app: &tauri::AppHandle, store: &Store, agent: &str, project: Option<&str>, group: Option<&str>, names: &[String]) -> Result<Vec<Skill>, String> {
    if !matches!(agent, "codex" | "deepseek") { return Err("不支持的 Agent".into()); }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).ok_or("找不到用户目录")?;
    let provider = if agent == "codex" { ".codex" } else { ".dsh" };
    let workspace = if agent == "deepseek" && group.is_none() {
        store.path.parent().ok_or("数据目录无效")?.join("runtimes/deepseek/workspace")
    } else {
        let id = store.resolve_chat(project, group)?;
        PathBuf::from(store.conversation_root(&id)?)
    };
    let mut roots = Vec::new();
    for dir in workspace.ancestors() {
        roots.push(dir.join(provider).join("skills"));
        roots.push(dir.join(".agents/skills"));
    }
    let variable = if agent == "codex" { "CODEX_HOME" } else { "DSH_HOME" };
    roots.push(std::env::var_os(variable).map(PathBuf::from).unwrap_or_else(|| home.join(provider)).join("skills"));
    roots.push(home.join(".agents/skills"));
    // This machine's personal skill installation lives in CODEX_HOME, including
    // linked directories. Share those files with DeepSeek without copying them.
    if agent == "deepseek" {
        roots.push(std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex")).join("skills"));
    }
    let mut skills = BTreeMap::new();
    let mut seen = HashSet::new();
    for root in roots { collect(&root, 0, &mut seen, &mut skills)?; }
    // Ask the provider rather than treating every cached plugin version as installed.
    if agent == "codex" {
        let catalog = app.state::<crate::codex::Runtime>().skills(store, &workspace)
            .map_err(|e| format!("读取 Codex 原生技能目录失败：{e}"))?;
        let data = catalog["data"].as_array().ok_or("Codex 技能目录格式不兼容")?;
        for entry in data {
            if entry["errors"].as_array().is_some_and(|items| !items.is_empty()) {
                return Err("Codex 报告技能加载错误，请检查本机技能配置。".into());
            }
            let items = entry["skills"].as_array().ok_or("Codex 技能列表格式不兼容")?;
            for item in items {
                let name = item["name"].as_str().ok_or("Codex 技能缺少名称")?.to_owned();
                if item["enabled"] == false { skills.remove(&name); continue; }
                let path = item["path"].as_str().ok_or("Codex 技能缺少路径")?.to_owned();
                skills.insert(name.clone(), Skill { name, path, description: item["description"].as_str().unwrap_or_default().into(), native: false, content: None, source: "Codex 运行时".into() });
            }
        }
    } else {
        let catalog = app.state::<crate::deepseek::Runtime>().skills(store, &workspace, names)
            .map_err(|e| format!("读取 DeepSeek 原生技能目录失败：{e}"))?;
        let items = catalog["skills"].as_array().ok_or("DeepSeek 技能目录格式不兼容")?;
        for item in items {
            let mut skill: Skill = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?;
            skill.source = "DeepSeek 运行时".into();
            skills.insert(skill.name.clone(), skill);
        }
    }
    Ok(skills.into_values().collect())
}
