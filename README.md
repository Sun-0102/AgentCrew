# AI-gent

面向个人用户的本地 AI 协作工作台。用户只打开 AI-gent，后台由 Codex 和 DeepSeek 执行任务。项目、私聊、协作、Git 和工作证据由 AI-gent 统一管理。

文档日期：2026-11-19。状态：暖色聊天工作台、Codex 私聊与本机群聊已实现。DeepSeek 和自动协作工作流待接入。AI-gent 为暂定项目名。

## 本地开发

需要 Node.js 22.12+、npm、Rust 1.90+，以及 [Tauri Windows 开发依赖](https://v2.tauri.app/start/prerequisites/#windows)（MSVC C++ 工具链和 WebView2）。

```powershell
npm ci
npm run desktop
```

第一次构建需要下载并编译 Rust 依赖。开发版使用 `local.ai-gent.desktop.dev` 应用标识，数据库写入 Tauri 解析的应用数据目录；界面右侧可查看实际位置。项目源码留在用户选择的原目录。

当前可用：Agent 导航、聊天/通讯录、搜索、创建本机群聊、邀请和管理自己的 Agent、引用回复、复制消息、@ 选择、文本附件、原生目录选择、项目切换与 SQLite 持久化。重复选择同一目录会选中已有项目。Codex 支持本机登录检测、真实模型目录、流式回复、工具记录、审批、取消和恢复。DeepSeek、自动多 Agent 工作流、Git 管理及托盘待实现。关闭窗口会退出程序并停止本软件管理的 Codex 进程组。

群聊可不关联项目，每群有独立上下文。邀请只处理自己的本机 Agent，没有外部邀请。DeepSeek 可先加入群，但不能回复或执行。群内可指定 Codex 执行；不会自动唤醒所有成员。文本附件最多三个，合计 60 KB，内容随消息发送。文字草稿可跨刷新恢复；引用和待发附件只在当前运行期间保留。界面及验证见 [设计说明](docs/06-界面设计.md) 和 [聊天工作台验收](docs/verification/2026-11-19-chat-ui.md)。

## Codex 私聊

1. 使用 Codex 桌面客户端已安装的运行组件，或安装 Codex CLI 并加入 PATH。AI-gent 检查 Windows 客户端的 `%LOCALAPPDATA%\OpenAI\Codex\bin` 和 PATH 中的运行文件，按实际版本选择较新版。使用 `codex login` 完成登录，或自行配置 Codex 支持的模型服务。
2. 打开软件默认进入 Codex 私聊并检测本机连接，无需添加项目。左侧 Codex 可随时返回；需要处理项目文件时，在“工作空间”添加并选择目录。“连接与设置”显示版本、实际路径和模型数量，不读取或复制登录密钥。
3. 选择模型并发送任务。模型更改从下一轮生效。回复和工具开始/完成记录在当前私聊显示。
4. 请求审批时，检查命令、目录或工具变更记录，再批准本次或拒绝。不支持的服务端请求返回明确错误，不自动批准。
5. 取消会请求 `turn/interrupt`，再清理 Windows Job Object 管理的整个后台进程组。只有收到中断终态并完成清理，才显示“已取消”；未确认结果显示“中断”。已产生的文件不会自动回退。

当前保留一段不绑定项目的 Codex 私聊，以及每个项目各自的一段会话。独立私聊使用 `~/Documents/Codex/AI-gent/<应用数据目录名>/codex-personal` 作为文件工作目录，不在项目列表中创建虚拟项目。旧版应用数据目录中的私聊文件会复制迁移，原目录保留，会话 ID 和消息不变。会话上下文和记录分别保存，整个应用一次执行一个任务。会话 ID、模型服务、运行版本、消息和工具记录保存在应用 SQLite 中。重启后可查看本地记录，下一条消息通过 `thread/resume` 恢复；版本或工作目录不匹配时停止恢复并报告错误。Codex 原生会话文件仍由 Codex 管理，因此恢复也依赖其本机历史。

已加入当前 Windows 官方客户端的“最近”兼容同步。它同时依赖无项目工作目录范围和会话来源筛选：独立 App Server 固定写入 `vscode` 来源，而本机官方客户端 26.1111 的侧栏扫描仅接收 `cli`。AI-gent 在启动恢复和每轮结束后，为自己创建的会话备份并修正索引分类与最近活动时间；真实创建方 `originator=ai_gent`、消息和执行权限均保留。原索引元数据备份在应用数据目录的 `sidebar-index-backups/<thread-id>.json`。这使用客户端内部 SQLite 索引兼容，不是稳定的官方同步 API；客户端升级、索引重建或用户自定义无项目目录后需重新验证。同步失败单独报告，不阻止普通私聊。

任务启动时请求 `workspace-write` 沙箱与 `on-request` 用户审批，并核对服务端返回配置。工作目录本身不是沙箱；不承诺限制全部读取范围。数据库现为版本 4，升级前自动生成 `*.v<旧版本>-<UUID>.sqlite` 备份，并保留原项目会话与消息。当前保存可读消息与部分工具事件，不是完整原始协议日志或离线证据包。

初次验证使用 PATH 中的旧版 `codex-cli 0.147.0-alpha.6.6`，仅返回 3 个普通模型。现已修正运行文件选择，本机选择客户端管理的 `codex-cli 0.169.0-alpha.11.1`，返回与客户端一致的 10 个普通模型。真实测试与未验收项见 [Codex 接入记录](docs/verification/2026-11-19-codex.md)。

```powershell
npm run build          # TypeScript 检查与前端生产构建
npm run check:rust     # Rust 编译检查
npm run test:rust      # 项目持久化、路径校验、数据库版本保护测试
npm run desktop:build # Windows 安装包构建（开发版标识）
```

普通 Rust 测试不调用模型。需要明确执行的真实探针：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib installed_codex_probe -- --ignored --nocapture
# 以下各项使用当前登录账号的模型额度，在临时目录内运行：
cargo test --manifest-path src-tauri/Cargo.toml --lib installed_codex_conversation_and_resume -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib installed_codex_cancel_tool -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib installed_codex_personal_chat_and_resume -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib installed_codex_group_chat -- --ignored --nocapture
```

`npm run dev` 仅用于浏览器预览，目录选择和 SQLite 需要桌面运行环境。正式发布前需配置独立应用标识、图标及发布验收。

Windows 构建前请先关闭正在运行的 AI-gent 窗口，避免可执行文件被占用。

## 当前代码结构

```text
src/                         React 界面
  features/projects/api.ts   项目与应用状态 IPC
  Workbench.tsx              聊天工作台、会话与项目导航
  features/chats/            群聊、成员、消息、模型、审批和状态
src-tauri/
  src/commands/              Rust 业务命令
  src/codex.rs               App Server stdio、进程组与会话恢复
  src/groups.rs              本机群、成员校验与会话隔离
  src/storage/               SQLite 与路径校验
  migrations/                数据库迁移
  capabilities/              窗口插件权限
docs/                        需求、设计与验收
```

## 已确认的决定

| 项目 | 决定 |
| --- | --- |
| 首个平台 | Windows 桌面优先 |
| 桌面技术 | Tauri 2、React、TypeScript、Vite、Rust |
| 首批 Agent | Codex、DeepSeek |
| Codex 接入 | Codex App Server |
| DeepSeek 接入 | DeepSeek Harness SDK 模式；用户已接受此路线 |
| 基础功能 | 选择项目、独立私聊、分别选择模型、协作群 |
| 数据归属 | 本地数据库和产物目录，不依赖某一家客户端查看记录 |
| Git | 软件调用 Git CLI，统一记录操作与代码版本 |

DeepSeek Harness 需要模型服务凭证，不复用 DeepSeek 聊天客户端的登录、记忆和历史。用户无需打开各家图形客户端，但后台仍需安装或配置匹配的 Agent 运行组件。

## 文档导航

| 文档 | 用途 |
| --- | --- |
| [产品需求](docs/01-产品需求.md) | 用户流程、第一版范围、功能要求 |
| [技术设计](docs/02-技术设计.md) | 架构、存储、进程、Git 和权限边界 |
| [Agent 接入与协作协议](docs/03-Agent接入与协作协议.md) | 两家接入、统一事件、状态机、消息和交接 |
| [开发计划](docs/04-开发计划.md) | 技术验证、开发顺序、阶段退出条件 |
| [验收清单](docs/05-验收清单.md) | 可复现的功能、失败恢复和发布检查 |

## 阅读顺序

产品讨论读产品需求。开发实施先读技术设计与接入协议，再按开发计划执行。验收逐项记录证据。

本组文档区分“用户已确认的需求”“建议的实现约定”“待技术验证的能力”。内部接口属于 AI-gent 设计，不是厂商现成 API。厂商 SDK、模型目录及权限能力以验证时的实际版本为准。

## 第一版成功标准

只打开 AI-gent，选择一个本地项目；分别与 Codex、DeepSeek 私聊并选择可用模型；创建协作群，完成需求拆解、实现、检查和修改；查看代码差异、执行结果及证据；重启后仍可查看完整工作记录。

## 第一版不包含

Claude、豆包接入，手机端，云端同步，多人协作，通用插件商店，控制原厂聊天窗口，导入全部原厂客户端历史，自动发布或无人确认地推送远端。

关闭窗口可选择最小化到托盘。彻底退出会停止本软件管理的执行；退出后常驻、系统重启后自动执行不属于第一版。
