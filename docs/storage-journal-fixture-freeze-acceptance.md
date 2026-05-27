# Storage and Journal Fixture Freeze Acceptance Evidence

This document records PR198 acceptance evidence for the controlled-beta storage and conversation journal baseline fixtures.

The policy source of truth is [storage-journal-fixture-freeze-policy.md](storage-journal-fixture-freeze-policy.md). This evidence file turns that policy into release-gate-checkable acceptance: the current baseline is named, mapped to compatibility tests, and tied to commands that must remain green before a beta release candidate is cut.

## Acceptance Scope

This acceptance covers the current controlled-beta baseline only. It does **not** approve a commercial-pilot storage or journal freeze.

Accepted controlled-beta baseline coverage:

- Storage root manifest can migrate from a legacy v0 fixture to the current `STORAGE_LAYOUT_VERSION`.
- Storage migration apply records a backup manifest path before rewriting the layout version.
- Reopened storage reports the current manifest version and the full current layout directory set.
- Conversation journal can replay a legacy v1 fixture that predates checksum fields.
- Conversation journal can append a current event after replaying the legacy fixture.
- Conversation journal replay remains deterministic for the fixture event order.
- Backup manifest and path integrity remain covered by storage tests and release gate workspace tests.
- Journal checksum, hash-chain, byte-count, event-count, and typed failure behavior remain covered by journal tests and release gate workspace tests.

Commercial-pilot freeze remains deferred until a pilot owner accepts this baseline as a long-lived compatibility contract and confirms rollback/restore evidence against a release candidate tag.

## Baseline Fixture Mapping

| Baseline area | Fixture / test | Evidence provided |
| --- | --- | --- |
| Storage layout migration | `crates/agentos-storage/tests/compatibility_fixtures.rs::old_storage_fixture_can_migrate_to_current_layout_and_reopen` | Legacy v0 manifest migrates to current layout, records backup manifest, and reopens through `AgentOsStorage::init`. |
| Conversation journal replay | `crates/conversation-journal/tests/compatibility_fixtures.rs::old_journal_fixture_can_replay_and_accept_new_appends` | Legacy v1 journal without checksum fields replays in order and accepts a new current-format append. |
| Backup manifest/path integrity | `cargo test -p agentos-storage backup` | Existing storage backup tests cover manifest verification, restore behavior, duplicate/unexpected/path traversal rejection, and integrity checks. |
| Journal integrity verification | `cargo test -p conversation-journal verify` | Existing journal verification tests cover checksums, segment metadata, hash-chain behavior, missing segment handling, and typed verification failures. |
| Release candidate guard | `./scripts/release-gate.sh` | Full workspace formatting, linting, tests, and required M25 docs checks remain green. |

## Required Future Updates

Any PR that changes persisted storage or journal shape must update this document in the same PR when it changes acceptance scope.

Examples that require an update:

- `STORAGE_LAYOUT_VERSION`, storage root layout, or manifest field changes.
- Backup manifest schema or path validation semantics changes.
- Migration metadata changes.
- Conversation event envelope shape changes.
- Journal segment manifest, checksum, hash-chain, compaction, or snapshot metadata changes.
- Artifact descriptor / integrity metadata changes that become part of storage baseline expectations.
- Audit export schema changes that are treated as persisted compatibility fixtures.

Each update must name the previous baseline, name the new baseline, add or update compatibility fixtures, and document rollback or backup expectations.

## Evidence Commands

Run these targeted commands for PR198-style fixture acceptance evidence:

```bash
cargo test -p agentos-storage --test compatibility_fixtures
cargo test -p conversation-journal --test compatibility_fixtures
cargo test -p agentos-storage backup
cargo test -p conversation-journal verify
```

Run the full release gate before cutting or tagging a controlled-beta release candidate:

```bash
./scripts/release-gate.sh
```

## PR198 Acceptance Result

Status: accepted for controlled beta.

Acceptance rationale:

- The fixture freeze policy exists and is linked from release, security, and beta posture docs.
- Current storage and journal compatibility fixtures are mapped to concrete targeted test commands.
- The release gate checks that this acceptance evidence exists and keeps its required sections.
- The acceptance remains deliberately narrower than commercial-pilot freeze.

Known limitations:

- The storage and journal baseline is not yet a commercial-pilot compatibility contract.
- Current fixture tests are generated in test code rather than stored as archived binary fixture folders.
- Host-level rollback/restore rehearsal against a tagged beta artifact remains a commercial-pilot blocker.
