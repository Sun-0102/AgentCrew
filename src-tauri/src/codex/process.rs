use super::*;

pub(super) fn sanitize_stderr_line(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    for secret_marker in [
        "bearer ",
        "authorization",
        "api_key",
        "apikey",
        "access_token",
        "refresh_token",
        "id_token",
        "cookie",
        "secret",
        "sk-",
    ] {
        if lower.contains(secret_marker) {
            return None;
        }
    }
    let mut cleaned: String = trimmed.chars().take(240).collect();
    if trimmed.chars().count() > 240 {
        cleaned.push_str("...");
    }
    Some(cleaned)
}

#[cfg(windows)]
pub(crate) struct ProcessJob(usize);

#[cfg(windows)]
impl ProcessJob {
    pub(crate) fn attach(child: &Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(format!(
                    "无法建立 Codex 进程组：{}",
                    std::io::Error::last_os_error()
                ));
            }
            let job = Self(handle as usize);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
            {
                return Err(format!(
                    "无法绑定 Codex 进程组：{}",
                    std::io::Error::last_os_error()
                ));
            }
            Ok(job)
        }
    }

    pub(crate) fn terminate(&self) -> Result<(), String> {
        if unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0 as _, 1) }
            == 0
        {
            return Err(format!(
                "停止 Codex 进程组失败：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as _);
        }
    }
}

#[cfg(unix)]
pub(crate) struct ProcessJob { group: i32, stopped: AtomicBool }
#[cfg(unix)]
impl ProcessJob {
    pub(crate) fn attach(child: &Child) -> Result<Self,String> {
        let group=child.id() as i32;
        if unsafe {libc::getpgid(group)}!=group {return Err("运行组件未进入应用管理的进程组。".into());}
        Ok(Self {group,stopped:AtomicBool::new(false)})
    }
    pub(crate) fn terminate(&self) -> Result<(),String> {
        if self.stopped.swap(true,Ordering::SeqCst) {return Ok(());}
        let term=unsafe {libc::kill(-self.group,libc::SIGTERM)};
        if term!=0 {
            if std::io::Error::last_os_error().raw_os_error()==Some(libc::ESRCH) {return Ok(());}
            self.stopped.store(false,Ordering::SeqCst);
            return Err("无法停止应用管理的进程组。".into());
        }
        std::thread::sleep(Duration::from_millis(200));
        if unsafe {libc::kill(-self.group,libc::SIGKILL)}!=0 {
            let err=std::io::Error::last_os_error().raw_os_error();
            // On macOS, killpg(SIGKILL) can return EPERM once the group has exited to a zombie state after SIGTERM.
            if !matches!(err,Some(libc::ESRCH)|Some(libc::EPERM)) {
                self.stopped.store(false,Ordering::SeqCst);
                return Err("无法清理应用管理的进程组。".into());
            }
        }
        Ok(())
    }
}
#[cfg(unix)]
impl Drop for ProcessJob {fn drop(&mut self) {let _=self.terminate();}}

pub(super) fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    #[cfg(unix)] {use std::os::unix::process::CommandExt;command.process_group(0);}
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    crate::process::apply_system_proxy(&mut command);
    command
}

pub(super) fn pick_codex(
    candidates: impl IntoIterator<Item = (PathBuf, String)>,
) -> Result<(PathBuf, String), String> {
    candidates
        .into_iter()
        .filter_map(|(path, version)| {
            let parsed = semver::Version::parse(version.strip_prefix("codex-cli ")?).ok()?;
            Some((parsed, path, version))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, path, version)| (path, version))
        .ok_or_else(|| {
            "找不到可用的 Codex。请安装 Codex CLI，或确认桌面客户端已安装运行组件。".into()
        })
}

pub(super) fn resolve_codex() -> Result<(PathBuf, String), String> {
    let name = if cfg!(windows) { "codex.exe" } else { "codex" };
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("PATH").and_then(|value| {
        std::env::split_paths(&value)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    }) {
        paths.push(path);
    }
    // Desktop updates can install a newer runtime without updating PATH.
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        if let Ok(entries) = std::fs::read_dir(
            PathBuf::from(local)
                .join("OpenAI")
                .join("Codex")
                .join("bin"),
        ) {
            paths.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path().join(name))
                    .filter(|path| path.is_file()),
            );
        }
    }
    #[cfg(target_os="macos")]
    {
        for app in ["Codex.app", "ChatGPT.app"] {
            paths.push(PathBuf::from("/Applications").join(app).join("Contents/Resources/codex"));
            paths.push(PathBuf::from("/Applications").join(app).join("Contents/Resources/codex-cli/bin/codex"));
        }
        for directory in ["/usr/local/bin","/opt/homebrew/bin"] {paths.push(PathBuf::from(directory).join("codex"));}
        if let Some(home)=std::env::var_os("HOME") {
            let home_apps = PathBuf::from(home).join("Applications");
            for app in ["Codex.app", "ChatGPT.app"] {
                paths.push(home_apps.join(app).join("Contents/Resources/codex"));
                paths.push(home_apps.join(app).join("Contents/Resources/codex-cli/bin/codex"));
            }
        }
    }
    paths.retain(|path|path.is_file());
    paths.sort();
    paths.dedup();
    let candidates = paths.into_iter().filter_map(|path| {
        let output = command(&path).arg("--version").bounded_output().ok()?;
        if !output.status.success() {
            return None;
        }
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Some((path, version))
    });
    pick_codex(candidates)
}
