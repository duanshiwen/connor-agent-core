# Connector and Browser Risk Review Templates

High-risk connector/browser PRs must reference this document and [security-review-checklist.md](security-review-checklist.md).

Security checklist sections: Connector, Browser Risk, Credential, Enterprise Permission

## Per-Connector Threat Review Template

```text
Connector name:
Connector owner:
Capability status: read-only / write / destructive / experimental
Auth scopes:
Data classes read:
Data classes written:
External side effects:
Irreversible side effects:
Rate-limit and retry policy:
Timeout policy:
Credential storage/ref usage:
Audit events emitted:
Telemetry exported:
Permission gates:
Redaction rules:
Test coverage:
Irreversible side-effect tests:
Accepted risks:
Commercial pilot decision: allowed / blocked / conditional
```

Review questions:

- Are all side effects declared before policy evaluation?
- Can retries duplicate irreversible operations?
- Can connector output leak restricted payloads into model context, telemetry, audit export, or debug bundles?
- Are provider credentials scoped to the minimum required connector/account/resource?
- Does offboarding or credential revocation stop future connector operations?

## Browser Automation Exposure Review Template

```text
Browser capability:
Owner:
Exposure status: disabled / host opt-in / default-on
User intent boundary:
Permission UX:
Human takeover behavior:
Navigation/click/paste policy:
Download/upload policy:
Credential entry policy:
Origin isolation assumptions:
DOM/screenshot/network trace retention:
Audit events emitted:
Telemetry exported:
Denied/default-fail behavior:
Test coverage:
Accepted risks:
Commercial pilot decision: allowed / blocked / conditional
```

Review questions:

- Can the browser perform destructive UI actions without explicit user intent?
- Are downloads/uploads treated as side effects?
- Are cookies, tokens, page text, screenshots, and DOM snapshots redacted or retained only with approval?
- Does human takeover pause automation safely?
- Is the host product UX clear enough for end users to understand risk?

## First Review: Gmail Read-Only Connector

Connector name: Gmail read-only connector  
Connector owner: Connector owner / host integration owner  
Capability status: read-only / beta conditional  
Auth scopes: read-only Gmail scopes only  
Data classes read: thread metadata and message metadata/content as allowed by host scope  
Data classes written: none  
External side effects: network reads to Gmail API  
Irreversible side effects: none expected  
Rate-limit and retry policy: host/provider policy required before commercial pilot  
Timeout policy: connector runtime timeout policy required before commercial pilot  
Credential storage/ref usage: OAuth credential ref via credential store boundary  
Audit events emitted: connector operation start/result should be emitted by host integration  
Telemetry exported: connector ID, operation kind, latency/error class only by default  
Permission gates: host must gate mailbox/account access before query/read  
Redaction rules: message bodies are not telemetry-safe by default  
Test coverage: existing connector-runtime read-only URL/auth boundary tests  
Irreversible side-effect tests: not applicable for read-only connector  
Accepted risks: provider-specific rate limit and production retry policy remain host-owned  
Commercial pilot decision: conditional; allowed only after host auth scopes and telemetry policy are documented

## First Review: Browser Kernel Current Capability

Browser capability: browser kernel read/interaction boundary  
Owner: Browser kernel owner / host product owner  
Exposure status: host opt-in only  
User intent boundary: required for click/paste/download/upload and destructive navigation flows  
Permission UX: not yet product-level complete; broad end-user exposure blocked  
Human takeover behavior: existing boundary must pause or stop automation according to host UX  
Navigation/click/paste policy: side-effectful actions require policy/permission coverage  
Download/upload policy: treated as external side effects  
Credential entry policy: do not automate credential entry without explicit host approval  
Origin isolation assumptions: host must define trusted origins and network trace retention  
DOM/screenshot/network trace retention: not telemetry-safe by default  
Audit events emitted: operation class and result, redacted  
Telemetry exported: operation class, latency, error class only by default  
Denied/default-fail behavior: fail closed when permission UX/policy is missing  
Test coverage: existing browser hardening tests  
Accepted risks: product-level permission UX remains incomplete  
Commercial pilot decision: blocked for broad exposure; conditional for internal/host-opt-in beta workflows
