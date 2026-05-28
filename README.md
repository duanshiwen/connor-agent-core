# Connor Agent Core

`connor-agent-core` 是 AgentOS 核心运行时边界的 Rust workspace，覆盖对话事件溯源、Agent 运行、Action 执行、审计、存储、身份、连接器、浏览器/内核实体，以及面向宿主应用的集成 API。

这个仓库是一个 kernel/runtime SDK，不是带品牌的产品 UI。它提供的是可嵌入到 macOS、桌面端、服务端或其他 AgentOS 客户端中的持久化契约、编排边界和宿主集成表面。

整个 workspace 采用有意分层的结构：领域 crate 定义稳定的数据与策略形状；运行时 crate 通过显式边界编排副作用；面向宿主的 crate 负责组合这些能力，但不会隐藏审计、权限、存储或生命周期决策。

## 设计原则

- **追加式对话历史**：对话状态变化以事件表示，并且可以确定性回放。
- **显式副作用边界**：外部副作用必须经过 `action-core` / `action-runtime`、策略检查和审计日志。
- **生产集成由宿主负责**：凭证、遥测导出、发布产物、原生通知投递、模型/服务提供商账号、OAuth 应用、签名、更新和产品 UX 都由宿主应用拥有。
- **不伪造生产成功**：测试适配器和需要宿主提供的集成，不应在生产路径中静默返回 placeholder 成功值。
- **默认可测试**：内存存储和 fake provider 用于确定性测试与示例。
- **稳定宿主 API 优先**：`client-substrate`、`agentos-client-bridge` 和 `agentos-kernel` 暴露主要的面向宿主组合边界。

## Workspace 地图

### 对话层

- `conversation-core`：ID、参与者、消息、事件、可见性、切片、Action/Run 生命周期类型。
- `conversation-journal`：追加式 journal 抽象，以及内存和分段 JSONL 实现。
- `conversation-kernel`：命令、投影器、状态、策略、快照和上下文切片构建。

### Agent 与模型层

- `agent-runtime`：当前 Agent run 处理器、上下文构建、工具/Action proposal 路由、重试/run/action 存储、approval queue 和 checkpoint。
- `client-substrate`：商业客户端 facade，提供 typed commands/events、UI projections 和保守的安全默认值。
- `agentos-client-bridge`：面向原生 binding 与宿主应用的 JSON-safe bridge 边界。
- `model-adapter`：模型 provider 抽象，以及 OpenAI-compatible / Anthropic adapter、streaming/tool call 支持、token budgeting 和 fake adapter。
- `assistant-core`：assistant profile、能力、偏好和对话辅助能力。

### Action、策略与审计层

- `action-core`：Action ID、schema、request、result 和 registry primitives。
- `action-runtime`：policy → executor → audit → conversation lifecycle 的编排。
- `capability-policy`：allow/ask/deny 策略评估、approval receipt、payload hash 和 policy 文件加载。
- `audit-log`：memory/JSONL/enterprise audit sink、审计查询、导出和完整性报告。
- `enterprise-permission-core`：企业用户、角色、授权、生命周期/离职流程、缓存和服务端权限存储。

### 宿主、存储与可观测性层

- `agentos-kernel`：面向宿主的组合根、runtime builder、service registry、host API、诊断和错误分类。
- `agentos-config`：typed config 解析、overlay、校验和脱敏。
- `agentos-storage`：持久化存储布局、artifact store、迁移、备份、锁和修复 primitive。
- `agentos-observability`：结构化 telemetry、metrics、redaction、JSONL export、diagnostics 和 pilot operations drill 类型。

### 领域与连接器层

- `entity-core`、`artifact-core`、`surface-core`、`asset-core`、`asset-index`：共享领域对象和索引。
- `browser-entity`、`browser-kernel-core`：浏览器 Action schema、权限 profile、面向 CDP 的 kernel domain 和 executor skeleton。
- `knowledge-entity`、`mail-entity`、`calendar-entity`、`reminder-core`、`notification-core`、`scheduler`：产品领域实体和确定性存储/executor。
- `identity-core`、`server-account-core`、`connector-runtime`：本地身份、凭证、服务端绑定、OAuth/provider 生命周期、连接器审计/离职边界。
- `device-pairing-core`、`sync-runtime`、`p2p-sync-runtime`：设备信任、sync manifest/merge 和 P2P sync 编排。
- `person-entity`、`relationship-core`、`people-intelligence`、`server-search-core`：人物、关系、搜索策略领域。

## 快速开始

```bash
cargo build --workspace
cargo test --workspace
```

运行宿主示例：

```bash
cargo run -p agentos-kernel --example minimal-cli-host
cargo run -p agentos-kernel --example minimal-server-host
cargo run -p agentos-kernel --example minimal-desktop-host
```

运行商业客户端示例：

```bash
cargo run -p client-substrate --example minimal-commercial-client-host
```

## 生产宿主职责

生产宿主必须为产品自有基础设施提供真实实现，而不是依赖 test-only 默认值：

- 持久化 conversation journal 和 storage root。
- 非 fake 的模型 provider 配置。
- 持久化或托管式 audit log。
- 系统 keychain、secure enclave 或后端 credential storage。
- 真实 OAuth app registration 和 connector authorization flows。
- Telemetry 与 crash report 的同意、导出、保留和删除策略。
- 适用场景下的原生通知、浏览器、更新、签名和 notarization 基础设施。
- 启用企业模式时的 enterprise admin、offboarding 和 remote revocation flow。

## 安全与隐私基线

- Production builder 会拒绝已知 test-only dependency declaration。
- Action 执行由 side-effect classification、capability policy、approval receipt 和 audit logging 共同约束。
- Diagnostics 默认排除凭证，并使用 redaction-aware export。
- Telemetry 和 crash reporting 是宿主拥有的、基于用户同意的流程。
- 需要宿主/provider 集成的 connector path 应显式失败，而不是返回 placeholder success data。
- Storage layout、migration、backup、repair 和 lock primitive 都是 compatibility-sensitive surface。

## Release Checklist / 发布检查清单

在发布或 review release candidate 前，从仓库根目录运行 release gate：

```bash
./scripts/release-gate.sh
```

Release gate 会验证：

1. README 中的 release checklist 仍然可发现。
2. 宿主示例可以编译。
3. `client-substrate` targets 和商业客户端示例可以编译。
4. API compatibility、client-substrate、bridge 和 security smoke gates 通过。
5. 格式检查通过：`cargo fmt --all --check`。
6. normal、test、example targets 的 lint 通过：`cargo clippy --workspace --all-targets -- -D warnings`。
7. 测试通过：`cargo test --workspace`。
8. 快速 commercial-client substrate smoke 通过：`./scripts/perf-smoke-gate.sh`。

更严格的本地 preflight：

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
./scripts/release-gate.sh
```

真实 provider 和 connector smoke tests 是可选项，因为它们需要外部账号和 secrets。只应在已经配置真实集成环境时运行：

```bash
./scripts/provider-smoke-gate.sh
./scripts/connector-smoke-gate.sh
```

`model-adapter` 中的真实 provider smoke tests 默认被 ignored，并且需要 provider-specific 环境变量。如果变量缺失，这些 ignored smoke tests 会打印 skip message，而不是失败。

## 最小 Conversation Kernel 示例

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

## Public API Stability Boundary / 公共 API 稳定边界

`0.1.x` 线当前稳定的 host/application integration boundary 直接维护在本 README 中，并由测试和 release gate 共同约束。

### Stable API / 稳定 API

稳定的 host-facing crates 包括：

- `client-substrate`
- `agentos-client-bridge`
- `agentos-kernel`
- `agent-runtime`
- `action-core`
- `action-runtime`
- `capability-policy`
- `audit-log`
- `agentos-storage`
- `agentos-observability`
- `identity-core`
- `enterprise-permission-core`

这些 API 可以进行 additive growth。破坏性签名或语义变化需要明确迁移路径、兼容性覆盖和 release note，除非该变更是安全/隐私修复所必需。

Serialized compatibility-sensitive surfaces 包括 bridge JSON payload、kernel event、client projection、storage manifest/migration、diagnostics bundle、approval receipt 和 policy decision。不兼容的序列化变更需要 schema/API version bump，并提供 compatibility 或 migration coverage。

### 半内部 API

`conversation-core`、`conversation-kernel`、connector/entity crates、people/relationship/search domains 和 browser kernel domains 等领域 crate 可供高级宿主使用；但产品客户端应优先使用 `client-substrate` 和 `agentos-client-bridge`，除非它们有意嵌入更底层的 kernel behavior。

### Unstable API / 不稳定 API

除非另有明确文档，下列内容均视为不稳定：

- crate 内部 module layout。
- test-only fake、fixture 和 helper constructor。
- experimental domain crate 和 connector implementation。
- audit export filtering、browser evidence extraction、diagnostics enrichment 和 provider-specific connector behavior 的具体 heuristic。

### Deprecation Policy / 废弃策略

Deprecated stable API 必须至少保留到后续一个 roadmap PR 之后，除非因为安全/隐私问题必须移除。不属于稳定 host-facing boundary 的 deprecated compatibility code，在当前 `agent-runtime` / `agentos-kernel` 覆盖和 release gates 通过后可以移除。旧的 event-consumer conversation runtime crate 已经移除；新集成应使用 `agent-runtime` 和 `agentos-kernel`。

## 测试策略

Workspace 使用分层测试：

1. 领域序列化与 invariant tests。
2. Journal/storage append、reload、fixture 和 integrity tests。
3. Projector 和 kernel command lifecycle tests。
4. Runtime、policy、action、approval 和 audit orchestration tests。
5. Host API、diagnostics、release-gate 和 example compile tests。
6. 使用 deterministic mocks 的 provider compatibility tests，以及可选 ignored real-provider smoke tests。

## 文档政策

顶层项目姿态、API 稳定性、安全/隐私基线和发布说明都维护在本 README 中。除非某个主题过大、确实无法在 README 中维护，并且具备明确 owner、lifecycle 和 release-gate reference，否则不要新增顶层 `docs/` 文件。

Schema-specific notes 应放在 `schemas/` 下对应 schema 旁边。Crate-specific notes 应放在对应 crate 的 README 或 rustdoc 中。

## License

Apache-2.0。参见 [LICENSE](LICENSE)。
