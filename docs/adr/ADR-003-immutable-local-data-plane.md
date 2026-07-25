# ADR-003: Immutable local data plane

## Status

Accepted.

## Decision

The first deployment uses SQLite and tenant-partitioned filesystem objects. Every repository method
requires authenticated tenant context. Corpus versions and published packets are append-only.
Storage is accessed through application ports so PostgreSQL and object storage can replace it.

## Consequences

Local operation remains self-contained and testable. SQLite does not provide hosted row-level
security, so composite tenant keys and negative isolation tests are mandatory.
