# Storage and Journal Beta Fixture Freeze Policy

This policy defines how persisted storage and journal formats evolve during controlled beta.

Security checklist sections: Storage formats, release/rollback

## Freeze Posture

Current storage and conversation journal formats are accepted as **controlled-beta baseline fixtures**, but they are not yet commercial-pilot frozen.

Commercial pilot freeze requires explicit approval after the beta fixture lifecycle is exercised at least once.

Current controlled-beta acceptance evidence is recorded in [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md).

## Baseline Fixture Contract

A persisted format fixture becomes a baseline when it represents a released beta tag and is referenced by compatibility tests.

Baseline fixtures must remain backward-readable for the supported beta line.

## When to Add a Fixture

Add a fixture when a PR changes any of the following persisted shapes:

- `STORAGE_LAYOUT_VERSION` or storage directory layout.
- Backup manifest structure or path semantics.
- Migration metadata.
- Conversation journal event envelope shape.
- Conversation journal segment/manifest structure.
- Artifact descriptor or integrity metadata.
- Audit export schema version.

## Required PR Checklist for Persisted Shape Changes

- [ ] Name the old and new schema/layout versions.
- [ ] Add or update migration logic when old data must be read by new code.
- [ ] Add a fixture representing the old released shape.
- [ ] Add replay/restore/migrate tests for the fixture.
- [ ] Document backup and rollback expectations.
- [ ] Add release note text for operators.
- [ ] Run `cargo test --workspace` and `./scripts/release-gate.sh`.

## Deprecating Fixtures

A beta baseline fixture may be deprecated only when:

1. The supported beta line is no longer supported, or
2. Maintainers explicitly approve dropping backward readability for a non-commercial pilot, and
3. Release notes call out the compatibility break.

Commercial-pilot fixtures cannot be removed without pilot approval.

## Storage Layout Rules

- New persisted fields must be optional or have deterministic defaults when read from older fixtures.
- Path entries in manifests must remain relative and must reject absolute paths, `..`, duplicate paths, and unexpected files.
- Backup restore must verify manifest integrity before copying data into a target root.
- Non-empty restore targets must be rejected unless a future explicit overwrite workflow is approved.

## Conversation Journal Rules

- Event envelope schema changes require replay compatibility tests.
- New event fields must preserve deterministic projection from older journals.
- Unknown or future versions must fail with typed errors rather than panic.
- Compaction/snapshot features must not destroy the only replayable event source during beta.

## Migration and Rollback Notes

Every persisted-shape PR must include operator-facing notes:

```text
Persisted format changed: yes/no
Old version:
New version:
Migration command/path:
Backup required: yes/no
Rollback supported: yes/no
Fixture added/updated:
Known limitations:
```

## Release Gate Relationship

`./scripts/release-gate.sh` must remain green after fixture changes. If a fixture update requires additional docs checks, update the release gate in the same PR.

The release gate also checks [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md) so the current controlled-beta baseline acceptance evidence cannot be dropped silently.
