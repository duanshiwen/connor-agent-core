# v2 Commercial Kernel Readiness Review

This review defines the current v2 commercial kernel readiness posture for `connor-agent-core`. It is a release-facing document, not a product roadmap. It summarizes what is ready, what remains open, and which conditions must be true before beta or commercial pilot usage.

## Readiness Report

The v2 kernel is ready for controlled beta hardening once the release gate remains green and the stable host-facing boundary stays limited to the documented crates:

- `agentos-kernel`: composition root, runtime lifecycle, host API, error taxonomy, and kernel service wiring.
- `action-runtime`: policy-to-execution action orchestration boundary.
- `audit-log`: audit query/export boundary, JSONL export, redaction, and permission-filtered export behavior.
- `enterprise-permission-core`: enterprise grants, resource permissions, lifecycle/offboarding semantics, and administrative permission checks.

Current readiness strengths:

- Kernel composition and host API examples exist for CLI, server-shaped, and desktop-shaped hosts.
- Release gate runs documentation checks, security checklist checks, feature matrix checks, formatting, clippy, and workspace tests with one command.
- Error taxonomy exposes stable host-facing categories and response shape.
- Audit export includes JSONL rendering, secret-like redaction, and permission-aware filtering.
- Enterprise permission semantics include user lifecycle/offboarding denial behavior.
- Compatibility fixtures cover storage migration and journal replay.
- Performance baselines cover conversation replay, knowledge search, and action runtime overhead with intentionally wide guards.
- Security review checklist exists for browser, credential, connector, and enterprise permission risk areas.

## Remaining Gaps

These items should be closed or explicitly accepted before commercial pilot:

- External connector implementations still need per-connector threat reviews and irreversible side-effect tests.
- Browser automation needs product-level permission UX before broad end-user exposure.
- Credential storage integration needs final host-specific storage decisions and operational rotation guidance.
- Observability currently has an in-memory sink boundary; production telemetry export policy and retention must be decided by the host product.
- Storage and journal compatibility coverage should grow with every new persisted fixture version.
- Release packaging, version tagging, changelog generation, and rollback runbooks are not yet automated beyond the current release gate.

## API Freeze Proposal

Freeze the following public boundary for beta with additive-only changes unless a documented deprecation path is followed:

- `agentos-kernel` host API types, runtime lifecycle entry points, stable error taxonomy, and `HostApiErrorResponse` shape.
- `action-runtime` request processing boundary, capability policy integration expectations, and audit/journal side-effect behavior.
- `audit-log` audit event/query/export types, JSONL export schema version, redaction expectations, and permission-filtered export semantics.
- `enterprise-permission-core` permission grant, role/action/resource checks, lifecycle/offboarding behavior, and admin permission model.

Allowed during beta:

- Additive fields with safe defaults.
- New enum variants only when callers can ignore or handle unknown values safely.
- Internal refactors that do not change documented behavior.
- Deprecations with migration notes and compatibility tests.

Not frozen:

- Internal module layout.
- Test-only fakes and fixtures.
- Experimental crates not named in the stable boundary.
- Product-specific host UX and packaging.

## Storage Format Freeze Proposal

Freeze storage and journal formats for beta only after compatibility fixtures cover the current on-disk shape.

Proposed beta freeze rules:

- Storage layout version changes require a migration implementation, fixture, rollback/backup expectation, and release note.
- Conversation journal event shape changes require replay compatibility tests for previous fixture versions.
- Checksums, manifests, and migration metadata must remain backward-readable for every supported beta fixture.
- New persisted fields must be optional or have deterministic defaults when read from older fixtures.
- Destructive migrations are not allowed during beta without explicit commercial pilot approval.

Current freeze posture:

- Storage format is not yet commercial-pilot frozen.
- Journal format is not yet commercial-pilot frozen.
- Both are acceptable for controlled beta if migrations and fixtures remain part of every storage/journal change.

## Beta Entry Conditions

The kernel can enter beta when all conditions are met:

- `./scripts/release-gate.sh` passes on the release commit.
- Stable boundary crates are documented in `docs/feature-matrix.md` and README public API boundary docs.
- High-risk PRs reference `docs/security-review-checklist.md`.
- No known critical gaps remain in permission denial, audit export redaction, storage migration, or host-facing error taxonomy.
- Minimal host examples continue to compile and run.
- Remaining gaps are either closed or explicitly accepted in this review.

## Commercial Pilot Entry Conditions

The kernel can enter commercial pilot only after beta conditions plus:

- API freeze proposal is accepted by maintainers and all breaking changes are deferred or documented with migration paths.
- Storage format freeze proposal is accepted and current storage/journal fixtures are treated as long-lived compatibility fixtures.
- Connector and browser risk reviews are complete for every enabled commercial connector.
- Credential storage, rotation, revocation, and offboarding runbooks are documented for the pilot host.
- Production observability export, redaction, retention, and access-control policy are documented.
- Release packaging, changelog, rollback, and incident escalation runbooks exist.
