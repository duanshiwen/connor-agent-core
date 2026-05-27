# Credential Operations Rehearsal

This rehearsal records evidence that the credential operations runbook can be mapped to existing kernel boundaries before commercial pilot.

Security checklist sections: Credential, Enterprise Permission

## Rehearsal Scope

Date: 2026-05-27  
Scope: controlled-beta rehearsal for credential rotation, device credential revocation, lifecycle denial, grant revocation, and stale-cache offboarding denial.

This is a code-level rehearsal using deterministic in-memory stores and existing tests. It does not claim a host product has rehearsed macOS Keychain prompts, upstream OAuth revocation, or production incident operations.

## Runbook Step Mapping

| Runbook operation | Kernel boundary / evidence | Status |
| --- | --- | --- |
| Rotate credential secret | `identity-core::rotate_credential_secret(...)` updates secret and timestamp while preserving `created_at` | Rehearsed |
| Revoke device credentials | `identity-core::revoke_device_credentials(...)` deletes only credentials scoped to the target device | Rehearsed |
| Deny offboarded user | `enterprise-permission-core::PermissionStore` denies offboarded users even with active grants | Rehearsed |
| Deny disabled user | `enterprise-permission-core::PermissionStore` denies disabled users | Rehearsed |
| Reactivate suspended user | `EnterpriseUserStatus` allows suspended → active and restores grant evaluation | Rehearsed |
| Revoke all direct grants | `PermissionStore::revoke_all_grants_for_user(...)` revokes direct grants deterministically | Rehearsed |
| Deny server-backed offboarded user | `ServerBackedPermissionStore` denies offboarded user after lifecycle update | Rehearsed |
| Deny stale cached enterprise access | stale grant cache cannot bypass offboarded lifecycle denial | Rehearsed |

## Commands Executed

```bash
cargo test -p identity-core rotate_credential_secret_updates_secret_and_timestamp
cargo test -p identity-core revoke_device_credentials_deletes_only_device_scoped_credentials
cargo test -p enterprise-permission-core permission_store_denies_offboarded_user_even_with_active_grant
cargo test -p enterprise-permission-core permission_store_denies_disabled_user
cargo test -p enterprise-permission-core permission_store_allows_suspended_user_after_reactivation
cargo test -p enterprise-permission-core permission_store_revoke_all_grants_for_user
cargo test -p enterprise-permission-core server_backed_store_denies_offboarded_user
cargo test -p enterprise-permission-core offboarded_user_cannot_access_enterprise_resources_with_stale_grant
```

## Result

All targeted rehearsal commands passed.

Observed evidence:

- `rotate_credential_secret_updates_secret_and_timestamp`: 1 passed.
- `revoke_device_credentials_deletes_only_device_scoped_credentials`: 1 passed.
- `permission_store_denies_offboarded_user_even_with_active_grant`: 1 passed.
- `permission_store_denies_disabled_user`: 1 passed.
- `permission_store_allows_suspended_user_after_reactivation`: 1 passed.
- `permission_store_revoke_all_grants_for_user`: 1 passed.
- `server_backed_store_denies_offboarded_user`: 1 passed.
- `offboarded_user_cannot_access_enterprise_resources_with_stale_grant`: 1 passed.

## Host-Level Pilot Rehearsal Evidence

PR202 upgrades the earlier code-level rehearsal into host-level commercial pilot evidence without binding the kernel to a specific product UI or cloud vendor.

| Host rehearsal item | Evidence recorded | Status |
| --- | --- | --- |
| Desktop host selected backend | Desktop host selected backend is macOS Keychain provider via the credential provider boundary; host owns service/account/access-group naming and OS prompt UX | Accepted for pilot host rehearsal |
| Backend service selected backend | Backend service selected backend is encrypted file credential store or service-managed secret storage behind `CredentialStore`; host owns key management/IAM and filesystem/secret-manager controls | Accepted for pilot host rehearsal |
| Rotation evidence | Existing `rotate_credential_secret_updates_secret_and_timestamp` proves replacement secret write preserves `created_at`, updates `updated_at`, and stays behind credential store boundary | Rehearsed |
| Revocation evidence | Existing `revoke_device_credentials_deletes_only_device_scoped_credentials` proves scoped revocation removes only targeted device credentials | Rehearsed |
| Offboarding evidence | Existing enterprise permission tests prove offboarded/disabled users fail closed even with active or stale grants | Rehearsed |
| Host audit expectation | Host audit expectation is metadata-only: actor, credential id, scope, backend type, operation, timestamp, provider response class, and redacted result; no plaintext secret/token material | Required for pilot operations |
| Diagnostics/telemetry redaction | Credential evidence must remain metadata-only and follow `production-observability-policy.md` redaction rules | Required for pilot operations |

Commercial pilot credential blocker disposition: PR202 closes the host backend selection and code-level rotation/revocation/offboarding rehearsal blocker for first backend/macOS integration. Real upstream OAuth revocation for enabled providers remains tracked under PR206, and connector-specific end-to-end offboarding remains tracked under PR208.

## Commercial Pilot Gaps Still Open

The following remain host/product rehearsal tasks:

- macOS Keychain prompt and access-group behavior.
- Encrypted file credential store key-management decision.
- Upstream OAuth token revocation against real providers.
- Host audit event emission for rotation/revocation operations.
- Incident-response evidence preservation and debug-bundle deletion workflow.
- End-to-end offboarding across connector-runtime, sync, and host account state.

## Pilot Entry Implication

Credential operations are acceptable for continued controlled beta hardening. Commercial pilot still requires a host-level rehearsal that uses the selected production credential backend and real provider revocation behavior.
