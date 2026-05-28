# Security Policy

`connor-agent-core` is a local-first AgentOS kernel and commercial client substrate. Security-sensitive production hosts must not use test-only runtime components.

## Production Guardrails

Production client substrate builds must provide explicit durable or managed dependencies for:

- conversation journal
- model adapter
- audit log
- storage root
- credential backend
- identity crypto

The production builder rejects declared fake or in-memory test components. See `ClientProductionDependencies` in `client-substrate`.

## Secrets and Diagnostics

- Raw credential material must stay in the host-selected credential backend.
- Diagnostic bundle plans default to excluding credentials and requiring secret scanning.
- Credential access audit events must include metadata only, never secret values.
- Debug bundles should expire and should be shared only after user or admin consent.

## Browser and Connector Safety

Browser automation and connector writes are high-risk side effects. Production hosts should keep irreversible or external mutations behind explicit approval unless an enterprise policy grants otherwise.

## Reporting

This repository is currently pre-commercial. Report vulnerabilities privately to the project owner before public disclosure.
