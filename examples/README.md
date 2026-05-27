# Kernel Host Examples

These examples are intentionally minimal. They do not implement product behavior; they only prove that the stable kernel host API can be integrated by different host shapes.

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
