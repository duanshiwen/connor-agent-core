//! AgentOS kernel composition root.
//!
//! This crate provides the first thin runtime container for composing the
//! existing AgentOS core services. It intentionally avoids implementing domain
//! behavior that belongs in lower-level crates.

mod builder;
mod diagnostics;
mod error;
mod event_store;
mod host_api;
mod registries;
mod runtime;
mod services;

pub use builder::KernelRuntimeBuilder;
pub use diagnostics::{
    AuditEventSummary, KernelDiagnosticsBundle, KernelFailureClassification, KernelFailureSummary,
    RecentAuditSummary, RedactedRuntimeConfig, StorageManifestDump,
};
pub use error::{KernelError, KernelErrorCategory, KernelResult};
pub use event_store::{
    CURRENT_KERNEL_EVENT_SCHEMA_VERSION, JsonlKernelEventStore, KernelAggregateRef,
    KernelEventActor, KernelEventCursor, KernelEventEnvelope, KernelEventId, KernelEventKind,
    KernelEventStore, KernelEventStoreError, KernelEventStoreResult, KernelProjectionSnapshot,
    KernelRedactionClass, MemoryKernelEventStore, group_events_by_aggregate,
};
pub use host_api::{
    HostActionDecisionRequest, HostActorContext, HostApiError, HostApiErrorResponse, HostApiResult,
    HostExecuteApprovedActionRequest, HostPendingApproval, HostPermissionResource,
    HostProcessActionRequest, HostRunStatus, HostRunStatusResponse, KernelHostApi,
    StartAgentRunRequest, StartAgentRunResponse, SubmitUserMessageRequest,
    SubmitUserMessageResponse,
};
pub use registries::{
    ActionExecutorRegistry, ConnectorRegistry, ConnectorService, ModelProviderRegistry,
    PolicyProviderRegistry, PolicyProviderService, RepositoryRegistry, RepositoryService,
    StorageProviderRegistry, StorageProviderService,
};
pub use runtime::{KernelHealthReport, KernelRuntime, KernelRuntimeState};
pub use services::{KernelActionRuntime, KernelServices};
