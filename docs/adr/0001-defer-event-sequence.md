# ADR 0001: Defer Per-Conversation Event Sequence

## Status

Accepted

## Date

2026-05-23

## Context

Conversation Kernel persists all conversation state changes as append-only events. The current event envelope contains:

- `schema_version`
- `event_id`
- `conversation_id`
- `occurred_at`
- `actor_id`
- `event`

A per-conversation monotonic `sequence: u64` would be useful for:

- stable replay ordering independent of file ordering
- segmented journal ranges
- snapshots such as `state-up-to-sequence-N`
- indexes by message, run, thread, and timestamp
- incremental replay after a known point
- concurrency control for multiple appenders

However, adding `sequence` correctly requires deciding who owns sequence assignment.

Possible ownership models:

1. **Kernel-assigned sequence**
   - Kernel must know the last sequence before append.
   - This couples command handling to storage internals.
   - Concurrent appenders become difficult without compare-and-swap semantics.

2. **Journal-assigned sequence**
   - Journal becomes the authoritative append sequencer.
   - The trait must accept a pending event and return the committed envelope.
   - This is likely the cleanest long-term model but requires changing the journal API.

3. **External coordinator-assigned sequence**
   - Useful for distributed or multi-process systems.
   - Too complex for the current local-first kernel.

The current implementation is local-first and single-writer-oriented. Replay order is the append order recorded in segment files and manifest segment ordering.

## Decision

Do **not** add `sequence` to `ConversationEventEnvelope` yet.

Add `schema_version` now, because it is low-cost and does not affect append ownership semantics.

Defer `sequence` until we intentionally redesign journal append semantics around committed events.

## Consequences

### Positive

- Avoids adding a misleading sequence field with unclear ownership.
- Keeps the current `ConversationJournal` trait simple.
- Prevents kernel code from depending on storage-specific ordering details.
- Leaves room for a cleaner journal-assigned sequence model later.

### Negative

- Snapshot and index work should not begin until sequence ownership is resolved.
- Multi-writer append safety is not yet specified.
- Event ranges are currently implicit in segment ordering, not explicit sequence ranges.

## Required Follow-Up Before Snapshot/Index Work

Before implementing snapshots, indexes, or concurrent append support, define and implement:

- [ ] `CommittedConversationEventEnvelope` or equivalent committed-event type.
- [ ] Journal append API that returns the committed envelope.
- [ ] Per-conversation monotonic sequence assignment.
- [ ] Concurrency behavior for simultaneous appends.
- [ ] Manifest segment ranges, e.g. `first_sequence` and `last_sequence`.
- [ ] Replay API by sequence range.

## Non-Goals

This ADR does not define the final sequence API. It only prevents premature introduction of an underspecified field.
