# Connor Agent Core

Connor Agent Core 是 AgentOS 运行时边界的 Rust SDK。它适合用在宿主应用里，为你的产品提供持久化对话、受策略约束的 Action 执行、模型与工具编排、审计日志、存储、身份、连接器，以及面向宿主的稳定 API。

它**不是**一个完整的产品 UI。它提供的是让 macOS 应用、桌面壳、服务端进程或嵌入式客户端安全运行 Agent 的基础能力；账号体系、凭证管理、遥测、原生交互、打包、发布和部署仍然由你的宿主产品负责。

## 你可以用它构建什么

- 一个 local-first 的 Agent 客户端，并拥有可追加、可回放的对话历史。
- 一个桌面或 macOS 宿主，在执行高风险操作前向用户请求确认。
- 一个服务端 Agent runtime，支持审计日志、可恢复的 run 和持久化状态。
- 一个商业客户端壳，使用稳定的 typed command、event 和 projection。
- 一个连接器体系，把凭证、OAuth、账号绑定和离职/offboarding 边界显式化。

## 安装与构建

在 workspace 根目录运行：

```bash
cargo build --workspace
cargo test --workspace
```

运行最小宿主示例：

```bash
cargo run -p agentos-kernel --example minimal-cli-host
cargo run -p agentos-kernel --example minimal-server-host
cargo run -p agentos-kernel --example minimal-desktop-host
```

运行商业客户端 substrate 示例：

```bash
cargo run -p client-substrate --example minimal-commercial-client-host
```

## 应该从哪个 crate 开始？

大多数 SDK 使用者应该从这些面向宿主的 crate 开始：

- `agentos-kernel`：组合根、runtime builder、service registry、host API、diagnostics 和稳定的宿主错误类型。
- `client-substrate`：商业客户端 facade，提供 typed command/event、UI projection、安全默认值和生产依赖校验。
- `agentos-client-bridge`：给原生 binding 和应用壳使用的 JSON-safe bridge。
- `agent-runtime`：更底层的 Agent run 处理器，负责 action proposal 路由、approval queue、retry、checkpoint，以及 run/action store。

如果你只是想把 runtime 嵌进一个宿主应用，优先看 `agentos-kernel`。如果你在做产品客户端界面，优先看 `client-substrate`，只有在需要更细粒度控制时再下沉到 `agentos-kernel`。

## 核心概念

### 对话是事件溯源的

对话变化会作为事件追加写入，并且可以确定性地回放成当前状态。这让历史可审计，也让崩溃或中断后的恢复更简单。

相关 crate：

- `conversation-core`
- `conversation-journal`
- `conversation-kernel`

### Action 会经过显式安全边界

外部副作用会经过 action schema、策略评估、approval receipt、执行器和审计日志。只读 Action 可以在策略允许时自动执行；写入型或高风险 Action 可以要求用户或管理员确认。

相关 crate：

- `action-core`
- `action-runtime`
- `capability-policy`
- `audit-log`

### 生产集成由宿主拥有

SDK 不会假装产品基础设施已经存在。生产环境需要由宿主提供真实依赖：模型 provider、持久化存储、凭证存储、OAuth 应用、遥测导出、原生通知、浏览器自动化、签名、更新和产品 UX。

开发和测试可以使用 deterministic、in-memory 或 static 组件。生产 builder 会拒绝已知的 test-only dependency declaration。

### Diagnostics 默认考虑隐私

Diagnostics 和 telemetry 类型围绕 redaction 和显式同意边界设计。宿主决定采集什么、导出什么、保留多久，以及如何删除。

相关 crate：

- `agentos-observability`
- `agentos-config`
- `client-substrate`

## 最小 Kernel 示例

```rust
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let journal = Arc::new(MemoryConversationJournal::new());
    let kernel = ConversationKernel::new(journal);

    let user = Participant {
        id: ParticipantId::from("user-1"),
        kind: ParticipantKind::Human,
        display_name: "User".to_string(),
    };

    let agent = Participant {
        id: ParticipantId::from("agent-1"),
        kind: ParticipantKind::Agent,
        display_name: "Assistant".to_string(),
    };

    let conversation_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Design conversation kernel".to_string()),
            participants: vec![user, agent],
            actor_id: Some(ParticipantId::from("user-1")),
        })
        .await?;

    let message_id = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conversation_id.clone(),
            sender_id: ParticipantId::from("user-1"),
            content: MessageContent::Text {
                text: "Help me design the event model.".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await?;

    let state = kernel.load_state(&conversation_id).await?;
    let slice = ConversationSliceBuilder::new(10)
        .build_recent_window(&state, &message_id)?;

    println!("slice messages: {}", slice.messages.len());
    Ok(())
}
```

## Workspace 地图

### Runtime 与宿主 API

- `agentos-kernel`：宿主组合根和 runtime API。
- `client-substrate`：商业客户端集成 facade。
- `agentos-client-bridge`：JSON-safe 的原生/应用桥接层。
- `agent-runtime`：Agent run 编排。
- `assistant-core`：assistant profile、能力和偏好类型。

### 对话、Action、策略与审计

- `conversation-core`、`conversation-journal`、`conversation-kernel`
- `action-core`、`action-runtime`
- `capability-policy`
- `audit-log`
- `enterprise-permission-core`

### 存储、配置与可观测性

- `agentos-storage`
- `agentos-config`
- `agentos-observability`

### 领域与连接器

- `asset-core`、`asset-index`、`artifact-core`、`entity-core`、`surface-core`
- `browser-entity`、`browser-kernel-core`
- `knowledge-entity`、`mail-entity`、`calendar-entity`、`reminder-core`、`notification-core`、`scheduler`
- `identity-core`、`server-account-core`、`connector-runtime`
- `device-pairing-core`、`sync-runtime`、`p2p-sync-runtime`
- `person-entity`、`relationship-core`、`people-intelligence`、`server-search-core`

## 生产宿主检查清单

在真正发布宿主应用之前，请提供并校验：

- 持久化 conversation journal 和 storage root。
- 非 test-only 的模型 provider 配置。
- 持久化或托管式 audit log。
- 系统 keychain、secure enclave 或后端 credential storage。
- 真实 OAuth app registration 和 connector authorization flow。
- Telemetry 与 crash report 的用户同意、导出、保留和删除策略。
- 适用场景下的原生通知、浏览器、更新、签名和 notarization 基础设施。
- 如果启用企业模式，需要 enterprise admin、offboarding 和 remote revocation flow。

## 安全与隐私基线

- 生产依赖校验会拒绝已知 test-only 组件。
- Action 执行会受到 side-effect classification、capability policy、approval receipt 和 audit logging 约束。
- Diagnostics 默认会对疑似 secret 的值做脱敏。
- Telemetry 和 crash reporting 是宿主拥有、基于用户同意的流程。
- 需要宿主或 provider 集成的 connector path 应显式失败，而不是返回 placeholder success data。
- Storage layout、migration、backup、repair、lock、bridge payload 和 approval receipt 都是 compatibility-sensitive surface。

## Release checks

维护者可以运行完整 release gate：

```bash
./scripts/release-gate.sh
```

机器可检查的 release 与 public API 契约位于 [`schemas/release-contract.toml`](schemas/release-contract.toml)。Gate 会直接检查这个契约，因此 README 可以保持面向 SDK 使用者，而不是承载测试断言。

常用本地 preflight：

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
./scripts/release-gate.sh
```

Provider 和 connector smoke tests 是可选项，因为它们需要外部账号和 secrets：

```bash
./scripts/provider-smoke-gate.sh
./scripts/connector-smoke-gate.sh
```

## API 稳定性

稳定的 host/application integration boundary 记录在 [`schemas/release-contract.toml`](schemas/release-contract.toml)。简要规则是：

- 面向宿主的 crate 可以做 additive growth。
- 破坏 stable API 的变更需要迁移路径和兼容性覆盖。
- 破坏序列化格式的变更需要 schema/API version bump。
- 安全或隐私修复在必要时可以移除兼容代码。

## License

Apache-2.0。参见 [LICENSE](LICENSE)。
