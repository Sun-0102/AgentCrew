use base64::{engine::general_purpose::STANDARD, Engine};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

const LOCAL_IMAGE_MAX_BYTES: usize = 20 * 1024 * 1024;

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

fn detect_image_mime(path: &Path, bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }
    if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        return Some("image/avif");
    }
    let is_svg_ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("svg"))
        .unwrap_or(false);
    if is_svg_ext {
        if let Ok(text) = std::str::from_utf8(bytes) {
            let trimmed = text.trim_start_matches('\u{feff}').trim_start();
            let lower = trimmed.to_ascii_lowercase();
            if (lower.starts_with("<svg") || lower.starts_with("<?xml")) && lower.contains("<svg") {
                return Some("image/svg+xml");
            }
        }
    }
    None
}

fn read_local_image(file_path: &str, workspace_path: Option<&str>) -> Result<String, String> {
    let path = resolve_path(file_path, workspace_path)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("无法访问图片文件 {}：{error}", path.display()))?;
    if !metadata.is_file() {
        return Err("目标路径不是图片文件".into());
    }
    if metadata.len() > LOCAL_IMAGE_MAX_BYTES as u64 {
        return Err("图片过大，超过 20 MB 限制".into());
    }
    let file = fs::File::open(&path)
        .map_err(|error| format!("无法读取图片文件 {}：{error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(LOCAL_IMAGE_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取图片失败：{error}"))?;
    if bytes.len() > LOCAL_IMAGE_MAX_BYTES {
        return Err("图片过大，超过 20 MB 限制".into());
    }
    let mime = detect_image_mime(&path, &bytes).ok_or("不支持的图片格式或文件损坏")?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[tauri::command]
pub async fn local_image_read(
    file_path: String,
    workspace_path: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        read_local_image(&file_path, workspace_path.as_deref())
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

    #[test]
    fn reads_local_images_and_rejects_non_image_files() {
        let dir = tempfile::tempdir().unwrap();
        let png_bytes = b"\x89PNG\r\n\x1a\nfake-png-data";
        let png_path = dir.path().join("preview.png");
        fs::write(&png_path, png_bytes).unwrap();
        let data_url = read_local_image("preview.png", dir.path().to_str()).unwrap();
        assert_eq!(
            data_url,
            format!("data:image/png;base64,{}", STANDARD.encode(png_bytes))
        );

        let svg_bytes = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        fs::write(dir.path().join("diagram.svg"), svg_bytes).unwrap();
        let svg_url = read_local_image("diagram.svg", dir.path().to_str()).unwrap();
        assert!(svg_url.starts_with("data:image/svg+xml;base64,"));

        fs::write(dir.path().join("secret.json"), b"{\"token\":\"abc\"}").unwrap();
        assert!(read_local_image("secret.json", dir.path().to_str()).is_err());
        assert!(read_local_image("missing.png", dir.path().to_str()).is_err());
    }
}
