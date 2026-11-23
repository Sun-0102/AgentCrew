use std::{
    io::Read,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

pub trait BoundedOutput {
    fn bounded_output(&mut self) -> std::io::Result<Output>;
}
impl BoundedOutput for Command {
    fn bounded_output(&mut self) -> std::io::Result<Output> {
        let mut child = self
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        #[cfg(any(windows, unix))]
        let job = match crate::codex::ProcessJob::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::other(error));
            }
        };
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let out = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut reader = stdout;
            reader.by_ref().take(1_000_001).read_to_end(&mut bytes)?;
            std::io::copy(&mut reader, &mut std::io::sink())?;
            Ok::<_, std::io::Error>(bytes)
        });
        let err = std::thread::spawn(move || {
            let mut reader = stderr;
            std::io::copy(&mut reader, &mut std::io::sink())
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                #[cfg(any(windows, unix))]
                let _ = job.terminate();
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "运行组件读取超时",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        #[cfg(any(windows, unix))]
        job.terminate().map_err(std::io::Error::other)?;
        let bytes = out
            .join()
            .map_err(|_| std::io::Error::other("读取运行组件失败"))??;
        let _ = err.join();
        if bytes.len() > 1_000_000 {
            return Err(std::io::Error::other("运行组件输出超限"));
        }
        Ok(Output {
            status,
            stdout: bytes,
            stderr: Vec::new(),
        })
    }
}

/// Inherits macOS system proxy settings for GUI-launched child processes when proxy env vars are absent.
pub fn apply_system_proxy(command: &mut Command) {
    #[cfg(target_os = "macos")]
    {
        if std::env::var_os("HTTPS_PROXY").is_some()
            || std::env::var_os("https_proxy").is_some()
            || std::env::var_os("HTTP_PROXY").is_some()
            || std::env::var_os("http_proxy").is_some()
        {
            return;
        }
        let Ok(output) = Command::new("/usr/sbin/scutil").arg("--proxy").output() else {
            return;
        };
        if !output.status.success() {
            return;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let value = |key: &str| -> Option<String> {
            text.lines().find_map(|line| {
                let (k, v) = line.split_once(':')?;
                if k.trim() == key {
                    Some(v.trim().to_owned())
                } else {
                    None
                }
            })
        };
        let http_url = if value("HTTPEnable").as_deref() == Some("1") {
            match (value("HTTPProxy"), value("HTTPPort")) {
                (Some(host), Some(port)) if !host.is_empty() && !port.is_empty() => {
                    Some(format!("http://{host}:{port}"))
                }
                _ => None,
            }
        } else {
            None
        };
        let https_url = if value("HTTPSEnable").as_deref() == Some("1") {
            match (value("HTTPSProxy"), value("HTTPSPort")) {
                (Some(host), Some(port)) if !host.is_empty() && !port.is_empty() => {
                    Some(format!("http://{host}:{port}"))
                }
                _ => None,
            }
        } else {
            None
        };
        let socks_url = if value("SOCKSEnable").as_deref() == Some("1") {
            match (value("SOCKSProxy"), value("SOCKSPort")) {
                (Some(host), Some(port)) if !host.is_empty() && !port.is_empty() => {
                    Some(format!("socks5://{host}:{port}"))
                }
                _ => None,
            }
        } else {
            None
        };
        let mut applied = false;
        if let Some(ref url) = http_url {
            command.env("HTTP_PROXY", url).env("http_proxy", url);
            applied = true;
        }
        if let Some(ref url) = https_url {
            command.env("HTTPS_PROXY", url).env("https_proxy", url);
            applied = true;
        }
        if let Some(url) = socks_url.or(https_url).or(http_url) {
            command.env("ALL_PROXY", &url).env("all_proxy", &url);
            applied = true;
        }
        if applied && std::env::var_os("NO_PROXY").is_none() && std::env::var_os("no_proxy").is_none() {
            command
                .env("NO_PROXY", "localhost,127.0.0.1,::1")
                .env("no_proxy", "localhost,127.0.0.1,::1");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = command;
}
