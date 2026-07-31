# StudyPulse Desktop

StudyPulse Desktop 是一个面向 macOS 的本地优先学习工作区。它使用 SwiftUI 提供原生桌面界面，以 Rust 核心统一管理 Workspace、学习数据、资料检索、Agent 工具、权限确认和备份恢复。

当前版本是 MVP，重点是一个安全、可审计的 **Workspace + Cloud AI Agent** 闭环，而不是 iOS StudyPulse 的完整 macOS 平移版。

## 当前能力

- 创建、打开和恢复本地 StudyPulse Workspace
- 管理基础任务，并保留与 iOS 兼容的数据字段
- 浏览 `Documents/` 与 `Notes/` 中的文字资料
- 使用 Cloud AI Agent 进行对话、资料检索和任务协作
- 支持 `chat`、`deep_solve`、`mastery`、`deep_research`、`question_lab`、`visualize` 六种 Agent 模式
- 以时间线展示 Agent 状态、文本流、工具请求、确认、取消和完成事件
- 对写入、执行和破坏性操作进行逐次权限确认
- 导入并验证 iOS `.studypulsebackup` schema 3/4 备份，支持 Replace / Merge 恢复
- 支持可选的 SearXNG 搜索和 Docker Runner 代码执行后端
- 通过本地化资源提供英文、简体中文、繁体中文、日文和韩文界面

## 架构

```text
macOS SwiftUI
     │
     │ UniFFI DTO / cursor-based events
     ▼
studypulse-ffi
     │
     ▼
studypulse-agent ─── studypulse-tools ─── studypulse-workspace
     │
     └────────────── studypulse-model-client
```

- `macOS/StudyPulseMac`：SwiftUI、MVVM、AppKit 文件面板、Security-scoped bookmark、Keychain 和界面本地化。
- `studypulse-ffi`：跨平台 façade，只传递简单 DTO，不承载业务规则。
- `studypulse-agent`：协调模型请求、工具调用、事件流、取消和用户输入。
- `studypulse-tools`：工具 schema、参数校验、权限级别和工具执行。
- `studypulse-workspace`：Workspace 路径安全、JSONL 数据、资料读取和备份事务。
- `studypulse-model-client`：Cloud AI HTTP/auth 协议、结构化响应解析和测试 Mock。
- `studypulse-runner`：可选的、独立的 HTTP 代码执行 Runner，适合放在受控容器中。

更完整的边界、数据格式、Agent 生命周期和恢复事务说明见 [`ARCHITECTURE.md`](ARCHITECTURE.md)。

## 环境要求

- macOS 15 或更高版本
- Xcode 26.6 或更高版本
- Rust 1.97.1（由 [`rust-toolchain.toml`](rust-toolchain.toml) 固定）
- 如需 Cloud AI：可用的 StudyPulse Cloud AI 账号
- 如需本地代码执行：Python 3；如需容器化执行：Docker

仓库不嵌入 Python，也不保存访问令牌。Cloud AI 登录产生的 access/refresh token 只存储在 macOS Keychain 中。

## 快速开始

### 1. 生成 Rust Core 和 Swift 绑定

```sh
./core/scripts/build-macos-core.sh
```

脚本会编译 `studypulse-ffi`，生成本地静态库、UniFFI C 头文件、module map 和高层 Swift 绑定。低层构建产物位于被 Git 忽略的 `core/target/`；可审查的高层绑定位于 `macOS/StudyPulseMac/Generated/`。

### 2. 编译 macOS 应用

可以打开 `StudyPulse Desktop.xcodeproj`，或使用命令行构建：

```sh
xcodebuild \
  -project "StudyPulse Desktop.xcodeproj" \
  -scheme "StudyPulse Desktop" \
  -configuration Debug \
  -derivedDataPath .derivedData \
  CODE_SIGNING_ALLOWED=NO \
  build
```

### 3. 使用 Cloud AI Agent

1. 创建或打开一个 Workspace。
2. 在 Agent 页面选择 **Sign In**，完成 Cloud AI 网页登录。
3. 输入学习目标或问题。
4. 在时间线中查看资料读取、工具请求和模型回复。
5. 当 Agent 请求写入或执行操作时，逐次选择 **Allow** 或 **Deny**。

所有写入仍然需要本地一次性确认；取消操作会中断模型等待、用户确认等待和后续工具执行。

## 可选服务配置

Deep Research 和代码执行模式可以通过环境变量配置：

| 变量 | 说明 |
| --- | --- |
| `STUDYPULSE_SEARXNG_URL` | SearXNG 服务地址，例如 `http://127.0.0.1:8080` |
| `STUDYPULSE_RUNNER_URL` | 可选 Docker Runner 地址，默认 `http://127.0.0.1:45891` |
| `STUDYPULSE_RUNNER_TOKEN` | Docker Runner bearer token |
| `STUDYPULSE_CODE_EXECUTION_BACKEND` | `local`（默认）或 `docker` |
| `STUDYPULSE_PYTHON` | 可选的 Python 3 绝对路径 |

默认的 `local` 后端只会在用户明确允许后启动本机 Python。它具备超时、输入输出大小和临时目录限制，但不是安全沙箱；需要更强隔离时应配置 Docker Runner，并设置 `STUDYPULSE_CODE_EXECUTION_BACKEND=docker`。

## 数据与安全边界

- Workspace 数据保留在用户选择的本地目录中；Workspace 元数据不记录绝对路径或平台授权句柄。
- Core 只接受 Workspace-relative 路径，并拒绝绝对路径、路径穿越、符号链接越界、隐藏目录遍历和超大文件。
- Tool Registry 不提供 shell、任意进程、删除或任意路径能力。
- 模型可以请求工具，但 Rust Core 始终负责 schema 校验、权限确认、执行和事件记录。
- Agent 运行事件追加保存到 `Agent/runs/<run-id>.jsonl`，便于恢复和审计。
- 不要把 token、密码、生产配置或本地 Workspace 数据提交到仓库。

## 验证命令

Rust Core：

```sh
cd core
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

macOS Core 和应用：

```sh
./core/scripts/build-macos-core.sh
xcodebuild \
  -project "StudyPulse Desktop.xcodeproj" \
  -scheme "StudyPulse Desktop" \
  -configuration Debug \
  -derivedDataPath .derivedData \
  CODE_SIGNING_ALLOWED=NO \
  build
```

构建通用 XCFramework（需要对应的 Rust targets）：

```sh
./core/scripts/build-macos-xcframework.sh
```

## 当前边界

Desktop 目前还没有实现 iOS StudyPulse 的完整学习闭环，包括 Today 聚合首页、成绩/科目/阶段、考试管理、错题本、学习计时、Time Investment、趋势分析、闪卡、健康数据、成就和系统级提醒等。具体缺口见 [`docs/IOS_FEATURE_GAPS.md`](docs/IOS_FEATURE_GAPS.md)。

Windows 客户端也尚未实现；当前代码只保留跨平台 Rust Core 和 UniFFI façade 的架构边界。

## 仓库结构

```text
.
├── macOS/StudyPulseMac/        # macOS SwiftUI 客户端
├── core/                       # Rust workspace 和 UniFFI Core
├── StudyPulse Desktop.xcodeproj
├── ARCHITECTURE.md             # 架构与数据安全说明
└── docs/                       # 开发指南与 iOS 功能差距
```

## 开发说明

修改 UniFFI 暴露的 record、enum 或方法后，请重新运行 `./core/scripts/build-macos-core.sh`，并检查生成的 Swift 接口是否与 Swift 客户端一致。

更详细的开发流程见 [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)。

## 许可证

当前仓库尚未声明开源许可证。除非仓库后续补充许可证文件，否则请不要将代码视为可自由再分发。
