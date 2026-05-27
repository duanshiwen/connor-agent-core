# Credential Operations Runbook

This runbook defines beta/commercial-pilot operating expectations for credential storage, rotation, revocation, and offboarding.

Security checklist sections: Credential

## Storage Ownership Decision

| Backend | Intended use | Constraints |
| --- | --- | --- |
| macOS Keychain provider | Preferred desktop host backend on macOS | Host owns service/account naming, access groups, and OS prompt UX |
| Encrypted file credential store | Local-first fallback and testable host integration | Requires host-owned key management; credential files must not be world-readable |
| Memory/fake providers | Unit tests and deterministic examples only | Forbidden for production identity/credential mode |

The kernel provides boundaries; the host product owns final backend selection and operational controls.

## Secret Handling Rules

- Never log secrets, OAuth tokens, passwords, cookies, or API keys in plaintext.
- Redact secret-like fields in audit logs, diagnostics, observability events, exports, examples, and tests.
- Debug bundles must include credential metadata only, never credential values or encrypted blobs unless explicitly approved for an incident response workflow.
- Test fixtures must use obviously fake values such as `fake-token` or `test-secret`.

## Rotation Procedure

Trigger rotation when:

- A credential expires or provider policy requires refresh.
- A connector account is re-authorized.
- A device is suspected compromised.
- A pilot operator requests periodic rotation.

Procedure:

1. Identify the `CredentialId`, scope, connector/account/device owner, and current backend.
2. Write the replacement secret through the selected `CredentialStore` boundary.
3. Preserve `created_at`; update `updated_at` and emit/record an audit event in the host layer.
4. Validate dependent connector or device flow with a read-only operation when possible.
5. If validation fails, roll back to the previous credential only if it has not been revoked by the upstream provider.
6. Record the result in the release/pilot operations log.

## Revocation Procedure

Trigger revocation when:

- A device is offboarded or trust is revoked.
- A user is offboarded, disabled, or loses access.
- A connector account is disconnected.
- An upstream provider reports compromise.

Procedure:

1. Mark the affected user/device/account as denied at the permission/lifecycle layer first.
2. Revoke or delete scoped credentials using the credential store boundary.
3. Invalidate connector token refresh caches and device trust caches.
4. Attempt upstream token revocation where supported by the provider.
5. Verify subsequent connector/device access attempts fail closed.
6. Record audit evidence without storing secret values.

## Offboarding Sequence

1. Set enterprise lifecycle to `Offboarded` or host-equivalent terminal state.
2. Revoke direct grants and invalidate permission caches.
3. Revoke device-scoped credentials for all known devices.
4. Revoke connector/account-scoped credentials.
5. Remove sync eligibility and block future token refresh.
6. Export necessary audit evidence using redacted audit export.
7. Confirm permission-aware search/export paths no longer return protected resources.

## Failure Handling

- Credential backend unavailable: fail closed for external side effects; allow metadata-only diagnostics.
- Token refresh failure: do not overwrite the last known credential unless the provider returned a successful replacement.
- Partial revocation: keep the permission layer denied and retry revocation asynchronously from the host scheduler.
- Debugging incidents: prefer metadata, timestamps, credential IDs, scope, and backend type over secret material.

## Operational Evidence

A beta/commercial-pilot host should retain:

- Credential backend selected for the host.
- Rotation/revocation timestamps and actor.
- Scope and affected resource IDs.
- Provider response class, redacted.
- Verification result.

Do not retain plaintext secrets or encrypted credential file contents in operational logs.
