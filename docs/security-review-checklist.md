# Security Review Checklist

High-risk PRs must reference this checklist in their PR description, design note, or roadmap entry. A PR is high-risk when it changes browser automation, credential handling, connector execution, enterprise permissions, audit/export behavior, storage formats, network boundaries, or host-facing security semantics.

Reviewers should copy the relevant section into the PR and mark every applicable item. Non-applicable items should be marked with a short reason.

## Browser Risk Checklist

Reference: [connector-browser-risk-review-templates.md](connector-browser-risk-review-templates.md)

- [ ] Browser automation changes preserve user intent boundaries and do not silently perform destructive UI actions.
- [ ] Navigation, click, paste, download, and file-upload paths have explicit permission or policy coverage where side effects may occur.
- [ ] Browser-captured content is treated as untrusted input and is not executed as code.
- [ ] Screenshots, DOM snapshots, logs, and debug output avoid leaking credentials, tokens, session cookies, or private document contents.
- [ ] Browser connector failures produce actionable errors without exposing sensitive page data.

## Credential Checklist

Reference: [credential-operations-runbook.md](credential-operations-runbook.md)

- [ ] Secrets, API keys, OAuth tokens, passwords, cookies, and credentials are never logged in plaintext.
- [ ] Secret-like fields are redacted in audit logs, diagnostics, observability traces, exports, examples, and tests.
- [ ] Credential storage has a clear ownership boundary and does not persist secrets in world-readable project files.
- [ ] Credential rotation, revocation, and offboarding behavior is documented or explicitly out of scope.
- [ ] Test fixtures use fake credentials only and cannot be confused with production tokens.

## Connector Checklist

Reference: [connector-browser-risk-review-templates.md](connector-browser-risk-review-templates.md)

- [ ] Connector actions declare side effects accurately before policy evaluation.
- [ ] Connector inputs are validated before reaching network, filesystem, browser, or external-system boundaries.
- [ ] Connector outputs are treated as untrusted and are sanitized before model/context reuse when needed.
- [ ] Connector retries, timeouts, and failure modes avoid duplicate irreversible side effects.
- [ ] Connector audit events include enough context for accountability without leaking restricted payloads.

## Enterprise Permission Checklist

- [ ] Permission checks are performed before reading, mutating, exporting, or searching protected resources.
- [ ] Offboarded, disabled, and suspended users are denied according to the lifecycle semantics.
- [ ] Admin-only operations have explicit role/action checks and tests for denial paths.
- [ ] Permission-aware search/export paths do not leak unauthorized resource identifiers, summaries, or metadata.
- [ ] Permission changes, offboarding, and audit/export behavior remain consistent under concurrency.

## Observability and Release References

- Production telemetry/export changes should reference [production-observability-policy.md](production-observability-policy.md).
- Release, rollback, packaging, or incident-process changes should reference [release-operations-runbook.md](release-operations-runbook.md).
- Storage or journal format changes should reference [storage-journal-fixture-freeze-policy.md](storage-journal-fixture-freeze-policy.md).
- Controlled-beta decisions should reference [m24-beta-hardening-decision.md](m24-beta-hardening-decision.md).

## Required PR citation

High-risk PRs must reference this checklist with one of the following forms:

```text
Security checklist: docs/security-review-checklist.md
Security checklist sections: Browser Risk, Credential, Connector, Enterprise Permission
```

If a high-risk PR intentionally defers an item, the PR must name the deferred item, explain why it is safe to defer, and link the follow-up issue or roadmap task.
