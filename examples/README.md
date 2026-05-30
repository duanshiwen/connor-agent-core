# Kernel Host Examples

These examples are intentionally minimal. They do not implement product behavior; they only prove that the stable kernel host API can be integrated by different host shapes.

## PR201 commercial pilot host integration evidence

PR201 uses these examples as release-gated host integration evidence for backend and macOS/desktop teams. The examples intentionally remain small, but they now cover the host-shaped paths needed for commercial pilot bootstrap: runtime startup, message submission, run start/status, action processing, approval handoff, diagnostics bundle export, local storage boundary, and host-selected credential backend documentation.

## minimal CLI host

File: [`minimal_cli_host.rs`](minimal_cli_host.rs)

Runs a small command-line host boundary that constructs a `KernelRuntime`, starts it, creates a conversation, and appends a user message through `KernelHostApi`.

Run with:

```bash
cargo run --example minimal-cli-host
```

## minimal server host

File: [`minimal_server_host.rs`](minimal_server_host.rs)

Runs a server-shaped readiness/error boundary. It checks runtime health and converts a host API error into `HostApiErrorResponse`, demonstrating the stable host-facing error contract without starting a real network server.

Run with:

```bash
cargo run --example minimal-server-host
```

## minimal desktop host boundary

File: [`minimal_desktop_host.rs`](minimal_desktop_host.rs)

Runs a desktop-shaped boundary that creates a conversation, queries pending approvals, and shuts the runtime down through `KernelHostApi`, demonstrating UI host integration points without desktop product code.

Run with:

```bash
cargo run --example minimal-desktop-host
```

## M2.3 knowledge sync bridge host flow

File: [`knowledge_sync_bridge_host.md`](knowledge_sync_bridge_host.md)

Documents the host-side flow for consuming backend `/api/v1/sync/events` responses through `agentos-client-bridge` or the `agentos-ffi` dylib. It covers pull/apply/persist/ack discipline, the C ABI functions, expected backend response shape, and reducer behavior for personal knowledge sync.
