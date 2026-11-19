# AI-gent

面向个人用户的本地 AI 协作工作台。用户只打开 AI-gent，后台由 Codex 和 DeepSeek 执行任务。项目、私聊、协作、Git 和工作证据由 AI-gent 统一管理。

文档日期：2026-11-19。状态：最小桌面框架已建立，Agent 尚未接入。AI-gent 为暂定项目名。

## 本地开发

需要 Node.js 22.12+、npm、Rust 1.90+，以及 [Tauri Windows 开发依赖](https://v2.tauri.app/start/prerequisites/#windows)（MSVC C++ 工具链和 WebView2）。

```powershell
npm ci
npm run desktop
```

第一次构建需要下载并编译 Rust 依赖。开发版使用 `local.ai-gent.desktop.dev` 应用标识，数据库写入 Tauri 解析的应用数据目录；界面右侧可查看实际位置。项目源码留在用户选择的原目录。

当前可用：桌面三栏界面、原生目录选择、项目切换、Rust IPC、SQLite 初始化和项目持久化。重复选择同一目录会选中已有项目。Codex、DeepSeek、会话、协作、Git 管理及托盘仍待实现；界面不会发送 Agent 请求。当前关闭窗口会退出程序。

```powershell
npm run build          # TypeScript 检查与前端生产构建
npm run check:rust     # Rust 编译检查
npm run test:rust      # 项目持久化、路径校验、数据库版本保护测试
npm run desktop:build # Windows 安装包构建（开发版标识）
```

`npm run dev` 仅用于浏览器预览，目录选择和 SQLite 需要桌面运行环境。正式发布前需配置独立应用标识、图标及发布验收。

Windows 构建前请先关闭正在运行的 AI-gent 窗口，避免可执行文件被占用。

## 当前代码结构

```text
src/                         React 界面
  features/projects/api.ts   项目与应用状态 IPC
src-tauri/
  src/commands/              Rust 业务命令
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
