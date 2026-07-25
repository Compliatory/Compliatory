# ADR-005: Deterministic packets, budgets and cursors

## Status

Accepted.

## Decision

Canonicalise digest inputs with JCS, hash with SHA-256 and use domain-separated identifiers.
Timestamps and request metadata never enter packet digests. Fragments are atomic. Built-in
tokenizers are `registry:o200k_base` and conservative `conservative:utf8-bytes-v1`.

Continuation cursors are canonical payloads authenticated with HMAC-SHA-256 and bound to tenant,
inputs and corpus versions.

## Consequences

Calls can be replayed and tampering is detected. Conservative estimation may return less content
than a model could technically accept.
