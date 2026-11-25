use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAction {
    Resolve,
    Reveal,
    Open,
}

fn resolve_path(file_path: &str, workspace_path: Option<&str>) -> Result<PathBuf, String> {
    if file_path.trim().is_empty() || file_path.contains('\0') {
        return Err("文件路径无效".into());
    }
    let candidate = PathBuf::from(file_path);
    let path = if candidate.is_absolute() {
        candidate
    } else {
        let root = workspace_path
            .filter(|root| !root.trim().is_empty())
            .ok_or("无法确定文件位置，请使用完整路径或先选择项目。")?;
        let root = Path::new(root);
        if !root.is_absolute() {
            return Err("项目目录必须是完整路径".into());
        }
        root.join(candidate)
    };
    path.canonicalize()
        .map_err(|error| format!("无法访问文件 {}：{error}", path.display()))
}

fn open_command(path: &Path, action: FileAction) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("/usr/bin/open");
        if matches!(action, FileAction::Reveal) {
            command.arg("-R");
        }
        command.arg(path);
        command
    }
    #[cfg(target_os = "windows")]
    {
        // Pass the path as one argument, without a shell or command interpolation.
        let mut command = Command::new("explorer.exe");
        let display = path.to_string_lossy();
        let display = display.strip_prefix(r"\\?\").unwrap_or(&display);
        if matches!(action, FileAction::Reveal) {
            command.arg(format!("/select,{display}"));
        } else {
            command.arg(display);
        }
        command
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        let target = if matches!(action, FileAction::Reveal) {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        command.arg(target);
        command
    }
}

#[tauri::command]
pub async fn local_file_action(
    file_path: String,
    workspace_path: Option<String>,
    action: FileAction,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_path(&file_path, workspace_path.as_deref())?;
        if !matches!(action, FileAction::Resolve) {
            let output = open_command(&path, action)
                .output()
                .map_err(|error| format!("无法打开文件位置或应用：{error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "文件操作失败：{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
        }
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths_with_spaces_and_app_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Agent Crew.app");
        std::fs::create_dir(&app).unwrap();
        assert_eq!(
            resolve_path("Agent Crew.app", dir.path().to_str()).unwrap(),
            app.canonicalize().unwrap()
        );
    }

    #[test]
    fn resolves_absolute_files_without_workspace() {
        let file = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(
            resolve_path(file.path().to_str().unwrap(), None).unwrap(),
            file.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn rejects_unresolved_or_invalid_paths() {
        assert!(resolve_path("relative.dmg", None).is_err());
        assert!(resolve_path("", None).is_err());
        assert!(resolve_path("file\0name", Some("/tmp")).is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_path("missing.dmg", dir.path().to_str()).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_reveal_flag_and_literal_path_arguments() {
        let path = Path::new("/tmp/a file;$(echo test).dmg");
        let reveal = open_command(path, FileAction::Reveal);
        assert_eq!(reveal.get_program(), "/usr/bin/open");
        assert_eq!(
            reveal.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new("-R"), path.as_os_str()]
        );
        let open = open_command(path, FileAction::Open);
        assert_eq!(open.get_args().collect::<Vec<_>>(), vec![path.as_os_str()]);
    }
}
