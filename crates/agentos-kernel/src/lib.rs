//! AgentOS kernel composition root.
//!
//! This crate provides the first thin runtime container for composing the
//! existing AgentOS core services. It intentionally avoids implementing domain
//! behavior that belongs in lower-level crates.

mod builder;
mod diagnostics;
mod error;
mod host_api;
mod registries;
mod runtime;
mod services;

pub use builder::KernelRuntimeBuilder;
pub use diagnostics::{
    AuditEventSummary, KernelDiagnosticsBundle, RecentAuditSummary, RedactedRuntimeConfig,
    StorageManifestDump,
};
pub use error::{KernelError, KernelResult};
pub use host_api::{
    HostActionDecisionRequest, HostActorContext, HostApiError, HostApiResult, HostPendingApproval,
    HostPermissionResource, HostRunStatus, HostRunStatusResponse, KernelHostApi,
    StartAgentRunRequest, StartAgentRunResponse, SubmitUserMessageRequest,
    SubmitUserMessageResponse,
};
pub use registries::{
    ActionExecutorRegistry, ConnectorRegistry, ConnectorService, ModelProviderRegistry,
    RepositoryRegistry, RepositoryService,
};
pub use runtime::{KernelHealthReport, KernelRuntime, KernelRuntimeState};
pub use services::KernelServices;
