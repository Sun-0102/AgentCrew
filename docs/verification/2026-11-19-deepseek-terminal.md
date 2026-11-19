# DeepSeek Windows 终端修复

## 结果

已修复本机 Harness `0.2.1-rc.2` 在 `workspace-write` 模式启动命令时的 `0xC0000142`（`3221225794`）。真实 DeepSeek 调用 `pwsh` 返回 `AI_GENT_PWSH_OK`。

## 原因与改动

Harness 的 Windows 受限令牌沙箱要求命令继承现有控制台。后台进程链隐藏窗口时丢失控制台，导致命令初始化失败。独立对照验证中，同一 PowerShell 在只读沙箱可运行，可写沙箱失败；给实际沙箱启动器附着隐藏控制台后，可写模式成功。

应用在自己的运行目录生成两个适配文件：

- `console-plugin.cjs`：通过 Cordis 的 `insert` 注册启动适配，只在 Windows 的沙箱 runner argv 中添加 `--require console.cjs`。保留原 runner、全部策略参数、工作区和临时目录授权。
- `console.cjs`：仅在没有控制台时分配并隐藏控制台，并恢复原始 stdin/stdout/stderr 句柄。JSON-RPC 管道继续工作，已有控制台不被隐藏或替换。

没有修改 Harness 安装文件、系统文件权限或沙箱令牌，没有降级成无沙箱运行。启动适配依赖已锁定的 Harness 版本；版本变化仍由原兼容性检查拒绝。

## 验证

| 检查 | 结果 |
| --- | --- |
| 真实 DeepSeek 模型调用 `pwsh` | 返回 `AI_GENT_PWSH_OK` |
| 原生沙箱在中文及空格路径写文件 | 文件内容为“中文写入成功” |
| 同一命令尝试写入工作区外的测试文件 | 拒绝；文件未创建 |
| 取消正在休眠的受限 PowerShell | 原进程句柄确认退出，后台进程组终止 |
| 管道句柄、隐藏控制台及启动器恢复测试 | 通过 |

显式真实环境检查入口：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml installed_deepseek_enabled_powershell -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml installed_deepseek_terminal_ -- --ignored --nocapture
```

第一项调用一次真实模型；第二项直接运行本机 Harness 沙箱，不调用模型。测试只读写自行创建的临时目录。该验证不代表所有终端命令或额外权限操作都可执行；原有权限边界继续生效。
