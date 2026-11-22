# Agent 接入与协作协议

版本：0.1。日期：2026-11-19。本文包含厂商已公开能力、AI-gent 内部设计和待验证项。内部方法名不可直接作为厂商协议调用。

2026-11-22 新增 [Agent 自助接入与持续协作改进方案](08-Agent自助接入与持续协作改进方案.md)。配对 HTTP 协议、邀请码、动态实例身份、适配器合同和投递恢复以该方案为准。本文厂商协议作为执行层参考继续保留；配对成功不能替代真实运行能力检查。本次主要实现已写入，未构建、未测试，待用户验证。


2026-11-22 实施记录：已在 `codex/agent-self-onboarding` 写入 Agent 成员表、schema 15、自助配对、实例执行路由、后台队列恢复、托盘与平台适配代码。未构建、未测试、未运行模型探针。数据映射和用户验收见 [自助接入实现说明](verification/2026-11-22-agent-onboarding.md)。Gemini、Claude 不开放配对；Mac 未验收。本文其余早期实现状态为历史记录，新行为以该实现说明和改进方案为准。

最新实现补充：已加入 v2 通用邀请与 Gemini CLI ACP v1 适配代码，不需用户手选类型；原成员和历史保留。源码支持 schema 16，未运行迁移或功能验收。本机没有 Gemini，未安装或实机调用；仅完成前端构建与 Rust 编译检查，未测试、未提交、未推送。详见 [通用邀请与 Gemini 实现说明](verification/2026-11-22-generic-pairing.md)，此前的 v1 状态作为历史记录保留。

## 1. 接入总览

| 项目 | Codex | DeepSeek |
| --- | --- | --- |
| 后台入口 | codex app-server | dsh --profile sdk |
| 通信 | JSON-RPC，首选 stdio | JSON-RPC stdio |
| 认证 | Codex 支持的登录与账号配置 | DeepSeek API 凭证及 endpoint 配置 |
| 项目 | 会话绑定 cwd 与实际权限配置 | 运行实例工作目录及 profile 配置 |
| 模型 | model/list 读取可用模型及思考强度 | 从 Harness 配置/可用目录读取模型，具体协议需验证 |
| 连续私聊 | thread/start、thread/resume、turn/start | SDK 会话创建、运行和持久恢复，方法以固定版本验证结果为准 |
| 工具能力 | 由 Codex 环境提供 | 由 Harness profile 提供 |

两家图形客户端不必打开。软件负责准备和运行后台组件，不重新实现 Codex 或 Harness 的 Agent 循环。DeepSeek 模型调用仍使用 API 凭证，不继承原聊天应用账号。

## 2. Codex 适配器

### 启动与模型

1. 检测实际可执行文件、版本和认证状态。
2. 启动 App Server，完成该版本要求的 initialize 握手和后续通知。
3. 使用 model/list 分页读取当前配置下可用模型；按返回能力生成选择器。
4. 使用 thread/start 传入项目 cwd、选定 model、sandbox 与审批策略。
5. 持久保存 thread ID。请求 turn/start 后读取消息、工具、审批和完成事件。
6. 恢复会话前核对项目路径、provider 与运行版本；失败时保留记录。

模型和思考强度取实际返回值，不写死产品型号。厂商支持每轮覆盖 model、effort 等配置时，模型更改从下一轮生效。默认不改变 provider；切 provider 若需要 fork，必须显式呈现。

### 审批与终态

审批请求映射为应用 approvals，响应绑定 provider request ID。收到文字流结束不能直接标记任务成功，应等待对应 turn 终态。进程退出、请求失败和用户取消分别归类。

具体中断方法与错误 schema 必须由 P0 技术验证记录；未知方法不能靠猜测发送。stdout 只作协议解析，stderr 独立收集诊断。

## 3. DeepSeek 适配器

### 选定路线

首选官方 dsh 的 sdk profile，直接通过 stdio 接入 Rust。不使用 dsh-tui 作为自动化协议，不通过网页输入框控制 Harness，也不额外套 Python 中转层。

官方 CLI 已说明：headless 用于单次任务；sdk 用于长连接 JSON-RPC；acp 是另一种自动化入口。第一版采用 sdk，headless 仅供冒烟检查，不作为连续私聊和完整流式协作的替代品。

### 必须验证的项目

| 能力 | 已知依据 | 开发前验证 |
| --- | --- | --- |
| Windows 后台启动 | 官方 SDK 文档列出 Windows x64 | 固定版本、运行组件、中文路径、退出方式 |
| 选定工作目录 | 官方示例传入 cwd/workspace | sdk profile 实际文件解析及工具权限范围 |
| 模型配置 | 官方模型配置支持 DeepSeek key 与模型目录 | Rust 可调用的目录查询与单会话模型配置方式 |
| 连续会话 | 官方 SDK 示例包含 session_id 和本地日志 | 多轮上下文、进程重启、会话恢复方式 |
| 流式与工具事件 | SDK profile 提供 RPC 服务 | 事件 schema、关联 ID、工具输出完整性 |
| 取消与审批 | 不从模式名称推断支持 | 请求中断、工具子进程状态、审批回调 |

不能把 Python SDK 示例中的 run 方法名直接当作线上的 RPC 方法。验证时阅读选定版本的 schema、源码或官方客户端，保存脱敏协议夹具。

若缺少可编程模型列表，可在适配器内查询相同 endpoint 的公开模型目录，再验证 Harness 能否实际调用；界面显示列表来源。不能用“模型目录存在”替代执行可用性检查。

若同实例无法可靠隔离不同会话的模型配置，则使用独立实例/profile；同会话切模型是否需恢复或重建需实测。profile 的工具集合也必须验证，不能因为存在 SDK 模式就假定包含全部桌面功能。

## 4. AI-gent 内部适配接口（设计）

以下是内部业务接口，不承诺厂商有同名方法：

```text
probe() -> version, auth_state, capabilities
list_models() -> model_catalog
create_session(project, workspace, model, permissions) -> session
resume_session(session_ref) -> session
start_turn(session, input, model_options) -> attempt_handle
cancel(attempt_handle) -> cancellation_status
resolve_approval(request_ref, decision) -> result
shutdown(instance_ref) -> result
event_stream(instance_ref) -> normalized_events
```

capabilities 至少声明 models、streaming、resume、model_switch、cancel、approval、tool_events、usage、sandbox 的支持状态。状态可为 supported、unsupported、unverified，不能统一返回 true。

内部输入包含 project_id、workspace_id、conversation_id、participant_id、task_id（私聊可空）、attempt_id、model 和有类型的消息/附件引用。凭证仅经专用配置传递，不混入普通 input。

## 5. 规范化事件（设计）

```json
{
  "schema_version": 1,
  "event_id": "evt_001",
  "sequence": 1,
  "project_id": "project_001",
  "conversation_id": "conversation_001",
  "participant_id": "participant_codex_001",
  "attempt_id": "attempt_001",
  "agent": "codex",
  "type": "message.delta",
  "observed_at": "2026-11-19T02:00:00Z",
  "provider_event_id": null,
  "payload": { "text": "正在检查项目" }
}
```

| 类型 | 含义 |
| --- | --- |
| session.ready | 会话已可接收请求 |
| attempt.started | 执行已确认开始 |
| message.delta / message.completed | 文字增量及完整消息 |
| tool.started / tool.output / tool.completed | 可观测的工具执行 |
| approval.required / approval.resolved | 等待审批及处理结果 |
| artifact.created | 产物索引已持久保存 |
| usage.updated | 厂商实际提供的用量；缺失时显示未知 |
| attempt.completed | 运行环境正常结束，不等于业务验收通过 |
| attempt.failed / attempt.cancelled | 明确失败或明确取消 |
| runtime.disconnected | 运行环境断开，实际结果可能未知 |

未知原始事件保存脱敏摘要，不使整个会话崩溃。来自其他 attempt 的迟到事件只能追加到原尝试，不能更新当前尝试状态。

## 6. 执行状态与任务状态

执行尝试状态：

```text
queued → starting → running ↔ waiting_approval
                      │
                      ├→ succeeded
                      ├→ failed
                      ├→ cancel_requested → cancelled
                      └→ interrupted
```

cancel_requested 不等于 cancelled。超时仍不能确认结果时标 interrupted，并提示核对文件和进程。succeeded 表示本次 Agent 运行正常结束。

任务状态：draft、ready、running、blocked、awaiting_review、completed、failed、cancelled。执行 succeeded 后进入 awaiting_review，检查交付物与验收条件后才能 completed。重试保留同一任务、新建 attempt。

终态只允许有效当前尝试推动；重复完成事件幂等处理。任务状态与相关关键事件在同一数据库事务中提交。

## 7. 群消息与交接

消息类型为 user_message、assignment、handoff、result、question、decision、status。发送者身份与内容分开存储。显示名不作为路由依据，使用 participant_id。

普通消息不触发所有成员。@ 转为定向 assignment；默认按消息和用户最新要求派发。发送前先持久记录，分配稳定 delivery_id，防止界面重连重复派发。断线后不能确认已接收的执行请求进入核对状态，不能盲目重发。

交接包至少包含：任务目标、用户约束、验收条件、工作区与代码版本、来源尝试、相关消息摘要、文件差异/产物引用、验证结果和未解决问题。不能自动夹带其他私聊或密钥。

两个 Agent 不共享同一个厂商会话。共享的是项目事实、选定消息和证据。检查者只读检查指定代码版本；需要修改时创建明确写入任务。

## 8. 错误和恢复

统一错误码建议：AUTH_REQUIRED、AUTH_FAILED、MODEL_UNAVAILABLE、RATE_LIMITED、RUNTIME_MISSING、PROTOCOL_INCOMPATIBLE、PERMISSION_DENIED、WORKSPACE_MISSING、GIT_CONFLICT、STORAGE_FAILED、EXECUTION_UNKNOWN。

只读目录查询可按退避重试。任务执行、提交、合并及回退不能无条件自动重试。错误提示应显示阶段、可操作的恢复方式及脱敏诊断引用。

## 9. 版本验证记录模板

实现阶段新增 docs/verification/ 记录，每次包含：日期、Windows 版本、Agent 版本、安装来源、协议版本、模型目录样例、认证方式、工作目录、测试步骤、真实事件样例、支持能力、已知限制、对应验收编号。不保存 token 或密钥。

官方参考：[Codex App Server](https://learn.chatgpt.com/docs/app-server)、[Harness CLI](https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md)、[Harness SDK 示例](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/guide/python-sdk.md)、[Harness 模型配置](https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/user/guide/providers.md)。

## 2026-11-19 默认群聊协作实现（未测试）

用户更新决定：取消每条消息的自动协作开关和固定两轮限制。群消息统一进入 group_tasks / group_task_inputs，私聊不进入群队列。schema 8 在迁移前备份 schema 7，原群、项目、聊天和任务记录保留，旧 max_turns 仅为历史字段，不再用于调度。

用户消息先单独记录为 user，然后按提及的成员入队；未提及则使用当前负责人或所选成员。运行中可继续发送，排队留言显示数量，并在当前轮正常完成后优先处理。新的用户指派会取代旧回复尚未派发的交接；按时间使用最新用户要求。每条输入只选择并投递一次，应用注入的工作资料仍标为 assignment，不伪装成用户原话。

Agent 完成一轮后，仅从当前群、执行者、本轮消息序号范围内的最后一条 assistant 回复解析正文提及，排除代码围栏、缩进代码、行内代码、显式引用和邮箱式片段。同名提及去重，自指不唤醒自己；不要求最后一行或特殊完成标记。收到提及的成员读取用户目标、新留言、群内近邻消息、来源结果和真实工具记录，再自行决定如何协作。没有对方提及时任务为 idle，不继续调用模型。用户可确认完成，也可直接发新消息。

继续保持 thread ID、可用 Codex turn ID 和当前序号核对，以及全应用顺序执行。每群每小时最多启动 100 次执行作为频率保护，触发后暂停并保留未执行留言，不限制单个任务只能修订几轮。停止时先禁止继续派发并取消未执行输入，再处理当前提供方取消；重启标中断且不重放。正在执行的一轮不会收到实时 steering，插话在该轮结束后处理，需要立即中断时使用停止。

记录保留当前 HEAD、项目范围内的 Git 状态和已跟踪差异、未跟踪路径、工具记录及来源消息。长度限制和缺失明确说明；这不是不可变文件快照或完整协议证据导出。新能力未运行构建或测试，待用户验证。
