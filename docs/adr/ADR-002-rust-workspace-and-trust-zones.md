# ADR-002: Rust workspace and trust zones

## Status

Accepted.

## Decision

Use a Rust workspace. Pure domain and application crates forbid `unsafe`; MCP, database and PDF
dependencies live in adapters; runtime wiring lives in `servers/`; human-controlled workflows live
in `tools/`. Public boundaries exchange owned, serialisable Rust values.

## Consequences

Untrusted parsers and protocols cannot leak implementation types into the governed core. More
crates and explicit ports add ceremony but permit local and hosted adapters to share one contract.
