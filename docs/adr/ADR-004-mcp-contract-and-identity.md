# ADR-004: MCP contract and authenticated identity

## Status

Accepted.

## Decision

Expose the three `reg://` resource families and five tools documented in the architecture contract.
The server targets MCP `2025-11-25`. Tenant and permissions are injected through `AuthContext`; any
tenant identifier in an argument or URI is rejected. STDIO uses a configured service identity.

## Consequences

The MCP surface is transport-independent. Streamable HTTP and OAuth can be added without changing
tool schemas, while administrative operations remain unavailable to agents.
