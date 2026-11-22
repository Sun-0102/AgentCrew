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
