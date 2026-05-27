# Connector and Browser Commercial Review Evidence

This document records the first release-gate-checkable review evidence for connector/browser commercial pilot readiness. It uses the templates in [connector-browser-risk-review-templates.md](connector-browser-risk-review-templates.md) and preserves the current conservative posture: Gmail read-only is conditional, broad browser exposure remains blocked.

Security checklist sections: Connector, Browser Risk, Credential, Enterprise Permission

## Evidence Scope

Date: 2026-05-27  
Scope: code-level evidence for Gmail read-only connector and Browser Kernel current capability.

This is not a host-product approval. It records kernel/crate-level evidence and remaining commercial pilot blockers.

## Gmail Read-Only Connector Review Evidence

Commercial pilot decision: conditional.

Allowed only when the host supplies:

- read-only Gmail scopes only;
- account/mailbox permission gates;
- production rate-limit, retry, and timeout policy;
- telemetry redaction consistent with [production-observability-policy.md](production-observability-policy.md);
- credential rotation/revocation/offboarding rehearsal evidence for the selected backend.

Evidence commands:

```bash
cargo test -p connector-runtime gmail_connector_service_kind
cargo test -p connector-runtime gmail_connector_is_authenticated
cargo test -p connector-runtime gmail_connector_credentials
cargo test -p connector-runtime gmail_connector_list_resources_unauthenticated
cargo test -p connector-runtime gmail_connector_list_resources_unsupported_kind
cargo test -p connector-runtime gmail_connector_get_resource_unauthenticated
cargo test -p connector-runtime gmail_connector_get_resource_returns_metadata
cargo test -p connector-runtime gmail_oauth_boundary_scopes
cargo test -p connector-runtime connector_oauth_refresh_provider_refreshes_store_backed_token
cargo test -p mail-entity mail_action_schema_side_effects_match_policy_contract
cargo test -p mail-entity mail_list_and_get_are_allowed_by_default_safe_policy
cargo test -p mail-entity mail_create_draft_requires_approval_by_default_safe_policy
cargo test -p mail-entity mail_send_is_denied_by_default_safe_policy
```

Evidence mapping:

| Risk question | Evidence | Result |
| --- | --- | --- |
| Is the connector service identity explicit? | `gmail_connector_service_kind` | Rehearsed |
| Does unauthenticated access fail closed? | `gmail_connector_list_resources_unauthenticated`, `gmail_connector_get_resource_unauthenticated` | Rehearsed |
| Are unsupported resource kinds rejected? | `gmail_connector_list_resources_unsupported_kind` | Rehearsed |
| Is read metadata returned through a typed resource boundary? | `gmail_connector_get_resource_returns_metadata` | Rehearsed |
| Are OAuth scopes explicit? | `gmail_oauth_boundary_scopes` | Rehearsed |
| Can store-backed OAuth refresh preserve the credential boundary? | `connector_oauth_refresh_provider_refreshes_store_backed_token` | Rehearsed |
| Do read/list mail actions remain read-only under policy? | `mail_action_schema_side_effects_match_policy_contract`, `mail_list_and_get_are_allowed_by_default_safe_policy` | Rehearsed |
| Do draft/send mail actions require approval or deny by default? | `mail_create_draft_requires_approval_by_default_safe_policy`, `mail_send_is_denied_by_default_safe_policy` | Rehearsed |

Open blockers before commercial pilot:

- host/provider production retry policy;
- host/provider timeout policy;
- host audit event implementation for connector start/result;
- provider-specific rate limit treatment;
- end-to-end offboarding across host account state and connector runtime.

## Browser Kernel Current Capability Review Evidence

Commercial pilot decision: blocked for broad exposure; conditional for internal/host-opt-in beta workflows.

Broad exposure remains blocked until product-level permission UX, origin retention policy, and irreversible side-effect tests are complete.

Evidence commands:

```bash
cargo test -p action-runtime --test cdp_browser cdp_browser_open_url_is_read_only_allowed
cargo test -p action-runtime --test cdp_browser cdp_browser_extract_content_is_read_only_allowed
cargo test -p action-runtime --test cdp_browser cdp_browser_click_element_requires_approval
cargo test -p action-runtime --test cdp_browser cdp_browser_fill_form_requires_approval
cargo test -p action-runtime --test cdp_browser cdp_browser_execute_js_requires_approval
cargo test -p action-runtime --test static_browser static_browser_extract_content_is_read_only_allowed
cargo test -p action-runtime --test static_browser static_browser_summarize_is_read_only_allowed
cargo test -p action-runtime --test orchestrator browser_extract_content_auto_executes_through_action_runtime
cargo test -p action-runtime --test orchestrator browser_open_url_requires_approval_through_action_runtime
cargo test -p action-runtime --test orchestrator browser_capture_snapshot_requires_approval_through_action_runtime
```

Evidence mapping:

| Risk question | Evidence | Result |
| --- | --- | --- |
| Are read-only browser actions allowed under safe policy? | `cdp_browser_open_url_is_read_only_allowed`, `cdp_browser_extract_content_is_read_only_allowed`, `static_browser_extract_content_is_read_only_allowed`, `static_browser_summarize_is_read_only_allowed` | Rehearsed |
| Do click/fill/execute-js operations require approval? | `cdp_browser_click_element_requires_approval`, `cdp_browser_fill_form_requires_approval`, `cdp_browser_execute_js_requires_approval` | Rehearsed |
| Does action runtime enforce approval for non-read browser flows? | `browser_open_url_requires_approval_through_action_runtime`, `browser_capture_snapshot_requires_approval_through_action_runtime` | Rehearsed |
| Can read extraction auto-execute through runtime? | `browser_extract_content_auto_executes_through_action_runtime` | Rehearsed |

Open blockers before commercial pilot:

- product-level permission UX;
- explicit human takeover semantics;
- download/upload policy tests;
- irreversible side-effect tests for click/paste/upload/download flows;
- origin isolation, DOM/screenshot/network trace retention, and debug-bundle handling policy implemented in host.

## Pilot Entry Implication

Connector/browser evidence is sufficient to continue controlled beta hardening.

Commercial pilot remains conditional/blocked as follows:

- Gmail read-only connector: conditional; host/provider policies and end-to-end offboarding evidence required.
- Browser Kernel broad exposure: blocked; host opt-in beta only until irreversible side-effect tests and product permission UX are complete.
