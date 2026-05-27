# Production Observability Export Policy

This policy defines what beta/commercial-pilot hosts may export from AgentOS observability boundaries.

Security checklist sections: Credential, Connector, Browser Risk, Enterprise Permission

## Scope

The kernel provides structured trace/metric boundaries, in-memory test sinks, and a production-like JSONL file sink for host-owned local export. A host product may choose one of the following export modes:

- `disabled`: no production export; local diagnostics only.
- `in-memory`: process-local troubleshooting and tests.
- `file`: local redacted trace/metric files owned by the host. PR203 adds `JsonlObservabilityFileSink`, which writes redacted `traces.jsonl` and `metrics.jsonl` files plus export metadata for diagnostics linkage.
- `otlp` or vendor exporter: allowed only when the host documents retention, redaction, and access control.

The kernel must not bind directly to a commercial telemetry vendor.

## Redaction Requirements

Never export these values:

- Credential values, OAuth access tokens, refresh tokens, passwords, cookies, API keys, private keys, or encrypted credential blobs.
- Full browser DOM/screenshot contents unless explicitly captured as a user-approved artifact.
- Raw email/document bodies from connectors unless the host has a separate content-export permission model.
- Unauthorized resource identifiers, summaries, or metadata from permission-denied paths.
- Model prompts or outputs that contain private user data unless the host explicitly classifies and allows that data class.

Allowed by default when non-sensitive:

- Event name, crate/component, severity, timestamp.
- Run/action/tool call IDs.
- Outcome categories and stable error taxonomy categories.
- Latency, retry count, timeout class, circuit-breaker state.
- Redacted connector/provider names and high-level operation kind.

## Retention

Default controlled-beta retention:

- In-memory sink: process lifetime only.
- Local file sink: maximum 14 days unless the host product documents a shorter or longer policy.
- Remote telemetry: maximum 30 days for beta unless approved by pilot data owner.
- Debug bundles: retained only for the incident lifetime and deleted after closure.

## Access Control

- Production telemetry access is an operator/admin capability, not a general user capability.
- Enterprise tenant/org identifiers must be partitioned so one tenant cannot inspect another tenant's telemetry.
- Exported telemetry must inherit the same confidentiality classification as the highest-risk included attribute.
- Incident response access must be logged by the host product.

## Component-Specific Rules

### Model Calls

- Export token counts, provider/model ID, latency, retry/error category.
- Do not export prompt, completion, tool arguments, or structured output payloads by default.

### Tool Loop / Action Runtime

- Export action kind, lifecycle event, policy decision category, approval ID, and outcome.
- Do not export raw action inputs or outputs unless the action schema marks fields as export-safe.

### Browser Operations

- Export navigation/click/download/upload operation class and result.
- Do not export DOM snapshots, screenshots, cookies, URLs containing tokens, or page text by default.

### Connectors / Sync

- Export connector ID, operation kind, latency, retry, and error class.
- Do not export message bodies, calendar descriptions, file contents, or provider tokens by default.

### Enterprise Permission / Audit

- Export permission decision category and stable resource type when allowed.
- Do not export denied resource metadata unless the audit export permission model allows it.

## Production-Like File Export Sink

PR203 provides `JsonlObservabilityFileSink` as the first production-like local export sink. Host products own the export root, filesystem permissions, cleanup job, and operator access policy.

Required behavior:

- Trace events are redacted before being appended to `traces.jsonl`.
- Metric samples are appended to `metrics.jsonl` without secret-bearing labels.
- Export metadata includes `export_mode = file`, trace path, metric path, retention days, and redaction status.
- Default file retention metadata is 14 days unless the host explicitly configures a different value.
- Diagnostics/debug bundles may link to the export metadata or recent trace summary, but must not inline plaintext secrets or raw connector/browser content.

Evidence command:

```bash
cargo test -p agentos-observability jsonl_file_sink_exports_redacted_traces_and_metrics
```

## Debug Bundle Attachments

Debug bundles may attach redacted trace summaries. Raw trace attachments require:

1. A named incident.
2. Host operator approval.
3. Secret scan/redaction pass.
4. Expiration date.
5. Audit record of who generated and accessed the bundle.

## Commercial Pilot Exit Criteria

Before commercial pilot, the host must document:

- Selected export mode and sink.
- Retention period.
- Access-control policy.
- Redaction test strategy.
- Incident/debug bundle workflow.
