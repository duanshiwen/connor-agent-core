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

## PR206 OAuth Provider Lifecycle Evidence

PR206 closes the provider-shaped OAuth endpoint, revocation, and credential-store offboarding boundary for enabled connectors. This remains provider-shaped evidence rather than a live Google/GitHub network call so the release gate is deterministic and does not require secrets.

Additional evidence commands:

```bash
cargo test -p identity-core revoke_oauth_credential_calls_provider_and_deletes_store_record
cargo test -p identity-core offboard_connector_account_revokes_credentials_and_refresh_fails_closed
cargo test -p identity-core oauth_lifecycle_audit_event_is_metadata_only
```

Evidence mapping:

| Risk question | Evidence | Result |
| --- | --- | --- |
| Is a provider token/revocation endpoint boundary explicit? | `OAuthProviderEndpointConfig` and `FakeOAuthTokenRevoker` | Rehearsed |
| Does revocation call the provider-shaped revoker before deleting the local credential? | `revoke_oauth_credential_calls_provider_and_deletes_store_record` | Rehearsed |
| Does account offboarding revoke connector-account credentials and fail closed on future refresh? | `offboard_connector_account_revokes_credentials_and_refresh_fails_closed` | Rehearsed |
| Are refresh/revocation/offboarding audit records metadata-only? | `OAuthCredentialLifecycleAuditEvent`, `oauth_lifecycle_audit_event_is_metadata_only` | Rehearsed |

PR206 disposition:

- Token refresh and revocation behavior are covered by deterministic provider-shaped fakes.
- Offboarded connector accounts cannot refresh after credentials are revoked from the store.
- OAuth lifecycle audit evidence is metadata-only and omits access/refresh token material.
- Gmail connector host audit and end-to-end connector-runtime offboarding remain tracked under PR208.

## PR207 Gmail Retry Timeout Rate-Limit Evidence

PR207 closes the Gmail read-only provider retry, timeout, and rate-limit policy gap with a deterministic policy boundary. It does not call the live Gmail API; host products still own actual HTTP client wiring, but retry decisions are now explicit and release-gated.

Additional evidence commands:

```bash
cargo test -p connector-runtime gmail_provider_retry_policy
```

Evidence mapping:

| Risk question | Evidence | Result |
| --- | --- | --- |
| Are timeout and transient provider errors classified as retryable? | `gmail_provider_retry_policy_retries_timeout_and_transient_errors` | Rehearsed |
| Does rate-limit handling honor provider `Retry-After` seconds? | `gmail_provider_retry_policy_uses_retry_after_for_rate_limits` | Rehearsed |
| Do authentication and invalid-request failures fail closed without retry? | `gmail_provider_retry_policy_fails_closed_for_auth_and_invalid_request` | Rehearsed |
| Does retry stop at a deterministic max-attempt boundary? | `gmail_provider_retry_policy_exhausts_at_max_attempts` | Rehearsed |

PR207 disposition:

- Gmail read-only retry/backoff policy is deterministic and capped.
- Timeout, transient provider failure, and rate-limit classes are explicit.
- Rate-limit `Retry-After` is preserved as metadata and preferred over exponential backoff.
- Authentication/credential failures and invalid requests are not retried, preserving fail-closed behavior.

## PR208 Gmail Host Audit and Offboarding Evidence

PR208 closes the Gmail read-only host audit and connector-runtime offboarding evidence gap with deterministic metadata-only audit shapes and a host lifecycle access gate. It does not call the live Gmail API; host products still own real account lifecycle wiring, but connector-runtime now exposes the auditable decision shape needed for commercial pilot integration.

Additional evidence commands:

```bash
cargo test -p connector-runtime gmail_connector_operation_audit_shape_is_metadata_only
cargo test -p connector-runtime gmail_read_records_start_and_result_audit_events
cargo test -p connector-runtime gmail_offboarded_account_access_is_denied_and_audited
```

Evidence mapping:

| Risk question | Evidence | Result |
| --- | --- | --- |
| Are Gmail connector operation audit records metadata-only? | `ConnectorOperationAuditEvent`, `gmail_connector_operation_audit_shape_is_metadata_only` | Rehearsed |
| Can a Gmail read produce host-facing start/result audit evidence? | `gmail_read_records_start_and_result_audit_events` | Rehearsed |
| Does an offboarded host account fail closed before connector access? | `evaluate_connector_account_access`, `gmail_offboarded_account_access_is_denied_and_audited` | Rehearsed |
| Are Gmail payloads and OAuth token material omitted from audit evidence? | `payload_redaction`, `credential_redaction` assertions | Rehearsed |

PR208 disposition:

- Gmail connector operation start/result/failure/denial audit shape is metadata-only.
- Gmail message content/snippets and OAuth token material are omitted from audit-shaped output.
- Host lifecycle status `Offboarded` or `Disabled` denies connector access before read execution.
- Offboarding denial records an auditable `Denied` event with redacted payload/credential evidence.
- Gmail read-only has now completed PR206 provider lifecycle, PR207 retry/timeout/rate-limit, and PR208 audit/offboarding evidence for first pilot conditional enablement.

Open blockers before commercial pilot:

- host product must wire its real account lifecycle source into `evaluate_connector_account_access`;
- host product must persist/export `ConnectorOperationAuditEvent` through its selected audit backend.

## Browser Kernel Current Capability Review Evidence

Commercial pilot decision: blocked for broad exposure; conditional for internal/host-opt-in beta workflows.

Broad exposure remains blocked until product-level permission UX, origin retention policy, and irreversible side-effect tests are complete.

PR199 expands code-level irreversible side-effect evidence for click/type, fill, upload, and download policy behavior. This narrows the test gap but still does not replace host/product UX review.

PR209 adds a commercial-pilot browser permission profile contract. The first commercial pilot default is `BrowserPilotExposure::Disabled`, which blocks all browser action kinds. Host opt-in broad exposure is allowed only when product permission UX, irreversible side-effect mapping, real CDP irreversible evidence, and host risk acceptance are all recorded.

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
cargo test -p browser-entity browser_action_schemas_side_effects_match_policy_contract
cargo test -p browser-entity register_browser_action_schemas_adds_expected_actions
cargo test -p browser-entity browser_pilot_profile
cargo test -p action-runtime --test orchestrator browser_click_and_type_require_approval_through_action_runtime
cargo test -p action-runtime --test orchestrator browser_fill_upload_and_download_are_denied_by_default_safe_policy
```

Evidence mapping:

| Risk question | Evidence | Result |
| --- | --- | --- |
| Are read-only browser actions allowed under safe policy? | `cdp_browser_open_url_is_read_only_allowed`, `cdp_browser_extract_content_is_read_only_allowed`, `static_browser_extract_content_is_read_only_allowed`, `static_browser_summarize_is_read_only_allowed` | Rehearsed |
| Do click/fill/execute-js operations require approval? | `cdp_browser_click_element_requires_approval`, `cdp_browser_fill_form_requires_approval`, `cdp_browser_execute_js_requires_approval` | Rehearsed |
| Are upload/download action schemas explicitly registered with conservative side-effect classification? | `browser_action_schemas_side_effects_match_policy_contract`, `register_browser_action_schemas_adds_expected_actions` | Rehearsed in PR199 |
| Does action runtime enforce approval for click/type paste-like UI mutations? | `browser_click_and_type_require_approval_through_action_runtime` | Rehearsed in PR199 |
| Does action runtime deny fill/upload/download by default safe policy and audit the side effect? | `browser_fill_upload_and_download_are_denied_by_default_safe_policy` | Rehearsed in PR199 |
| Does action runtime enforce approval for non-read browser flows? | `browser_open_url_requires_approval_through_action_runtime`, `browser_capture_snapshot_requires_approval_through_action_runtime` | Rehearsed |
| Can read extraction auto-execute through runtime? | `browser_extract_content_auto_executes_through_action_runtime` | Rehearsed |
| Is browser broad exposure blocked by default for the first commercial pilot? | `BrowserPilotPermissionProfile::first_commercial_pilot_default`, `browser_pilot_profile_blocks_broad_exposure_by_default` | Rehearsed in PR209 |
| Does host opt-in require product UX, irreversible side-effect mapping, real CDP evidence, and risk acceptance? | `browser_pilot_profile_requires_all_product_gate_evidence_to_enable` | Rehearsed in PR209 |

## PR209 Browser Pilot Permission Profile Evidence

PR209 disposition:

- First commercial pilot default: `BrowserPilotExposure::Disabled`.
- Default profile blocks every registered browser action kind, including `browser.open_url`, `browser.upload_file`, and `browser.download_file`.
- `BrowserPilotPermissionProfile::host_opt_in` remains blocked unless all four evidence gates are true:
  - product permission UX ready;
  - irreversible side-effect mapping ready;
  - real CDP irreversible evidence ready;
  - host risk acceptance recorded.
- Browser broad exposure can therefore remain out of the first pilot while Gmail read-only and non-browser kernel capabilities proceed.

Open blockers before browser broad commercial exposure:

- product-level permission UX;
- explicit human takeover semantics;
- host/product review for download/upload policy UX;
- broader irreversible side-effect tests for real CDP download/upload, origin isolation, retention, and human takeover flows;
- origin isolation, DOM/screenshot/network trace retention, and debug-bundle handling policy implemented in host.

## PR210 Deferred Real CDP Evidence Decision

PR210 real CDP download/upload, origin, retention, and human takeover evidence remains deferred for the lean first commercial pilot because browser broad exposure remains disabled by PR209.

Decision:

- browser broad exposure remains disabled for the first lean pilot;
- PR210 remains deferred and is not required for Gmail read-only pilot readiness;
- any future decision to enable browser broad automation must reopen PR210 and complete product permission UX, real CDP irreversible side-effect evidence, retention/debug-bundle handling, and host risk acceptance before exposure.

## Pilot Entry Implication

Connector/browser evidence is sufficient to continue controlled beta hardening.

Commercial pilot remains conditional/blocked as follows:

- Gmail read-only connector: conditional; host/provider policies and end-to-end offboarding evidence required.
- Browser Kernel broad exposure: blocked; host opt-in beta only until irreversible side-effect tests and product permission UX are complete.
