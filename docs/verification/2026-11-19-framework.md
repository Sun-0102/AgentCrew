# 最小桌面框架交付记录

日期：2026-11-19。

## 已实现

- Tauri 2、React、TypeScript、Vite、Rust 工程及依赖锁文件。
- 项目导航、工作区、执行与证据三栏界面。
- 原生目录选择；Rust 校验目录存在、可读并规范化路径。
- SQLite 项目登记、重复目录复用、项目列表与切换。
- 数据库存储在开发版系统应用数据目录，界面可查看实际位置。
- 独立开发版应用标识，受限的目录选择插件权限。
- Agent 未接入状态、浏览器预览提示、加载状态和错误提示。

## 已完成的检查

以下检查在用户要求自行测试之前完成：

- `npm run build`：通过。
- `cargo build --manifest-path src-tauri/Cargo.toml`：通过。
- `cargo test --manifest-path src-tauri/Cargo.toml`：3 项通过，覆盖项目持久化、重复目录、中文与空格路径、无效路径及新版本数据库保护。
- `npm run desktop`：启动成功；桌面控件树显示 Rust 已连接、SQLite 已初始化。

桌面截图捕获失败，错误为 `FrameArrived timed out: timed out waiting on channel` 和 `window capture timed out: timed out waiting on channel`。控件点击也因 `coordinate input geometry is unavailable` 未能完成。未完成视觉验收及原生目录选择的端到端人工检查，不将其记录为通过。

用户已要求自行测试。后续不继续自动测试。安装包、完整 UI 流程及 Agent 接入留待后续验收。

交付时已启动的 `tauri build --debug --no-bundle` 未完成：Windows 因桌面程序仍在运行，拒绝替换 `target/debug/ai-gent.exe`（os error 5）。未继续重试，保留运行窗口供用户检查。再次构建前需关闭窗口。

## 版本

- Node.js 24.19.0，npm 11.17.0。
- Rust / Cargo 1.98.0。
- React 19.3.1，TypeScript 5.9.3，Vite 7.3.8。
- Tauri Rust 2.12.7，Tauri CLI 2.12.6，JS API 2.12.5，Dialog 2.8.4。
- rusqlite 0.38.0，使用 bundled SQLite。

## 当前边界

尚未实现 Agent 接入、会话、模型选择、协作调度、Git 管理、证据采集、托盘和发布安装包。当前关闭窗口即退出程序。下一步在现有框架中接入 Codex App Server。
