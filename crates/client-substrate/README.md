# client-substrate

Commercial client facade for AgentOS host integration.

## API Version

Current stable substrate API version: `1`.

Breaking changes to command/event/projection semantics must bump `CLIENT_SUBSTRATE_API_VERSION` and update compatibility tests.

## Modes

- `Test`: deterministic memory/fake defaults.
- `Development`: local development defaults.
- `Production`: requires explicit `ClientProductionDependencies` and rejects declared fake/in-memory components.

## UI Contract

Client UIs should consume:

- `events_after(cursor)`
- `conversation_list_projection()`
- `timeline_projection(conversation_id)`
- `run_projection()`
- `approval_projection()`

instead of reconstructing state from low-level kernel internals.

## Native Bridge

Use `agentos-client-bridge` for JSON-safe native host integration. The bridge is intentionally narrow and can later be wrapped by UniFFI, C ABI, or Swift Package tooling.
