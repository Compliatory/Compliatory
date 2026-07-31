# ADR-007: Immutable writes and schema evolution

## Status

Accepted.

## Decision

This ADR narrows ADR-003. ADR-003 chose SQLite with tenant partitioning behind replaceable ports and
declared corpus versions and packets append-only; this ADR makes the write and schema-evolution
mechanics concrete and normative for the storage adapters.

Regulatory data-plane writes are insert-only. Once a row exists under a tenant-scoped primary key it
is never updated or deleted through the repository; new content always arrives as new rows under new
immutable keys, meaning a new `corpus_version` or a new content-derived `fragment_id`. Re-submitting
byte-identical canonical content under an existing key is a no-op success, so fixture re-seeding and
ingestion retries are safe. Submitting different canonical content under an existing key is rejected
with `ErrorCode::ImmutableConflict` (wire code `immutable_conflict`), a new variant of the core
error enum introduced by the implementing change, not by this ADR.

Where the conflict check must actually compare content depends on the entity. Fragments and packets
derive their identifiers from a domain-separated SHA-256 digest of their canonical content
(`frag_`/`pkt_` over `fragment-v1`/`work-packet-v1`), so key equality already implies content
equality and a conflict is a hash-collision-level non-event; the invariant is asserted, not relied
upon. `corpus_versions` is keyed by `tenant_id` plus an operator-assigned `corpus_version`, so its
content axis — `source_digest` and `status` — must be compared explicitly and a differing
`source_digest` under an existing version is a conflict. `profiles` is keyed by `tenant_id`,
`profile_id` and `version`, all human-assigned, so the canonical profile body must be compared.
`relations` is a pure composite key with no separate content axis and is therefore trivially
idempotent.

Publication is transactional. The fragments of a candidate corpus, its `corpus_versions` row and the
ingestion transition to `published` commit inside a single SQLite transaction; a failure at any step
leaves no trace of the candidate, and no fragment of an unpublished corpus is ever readable.
Replaying a completed publication returns the existing published record instead of erroring or
writing twice, and concurrent attempts on the same corpus serialise to one consistent outcome.

Schema evolution replaces today's single `CREATE TABLE IF NOT EXISTS` batch with ordered, numbered
migrations tracked in `PRAGMA user_version`. Each migration runs in its own transaction and rolls
back completely on failure. A database whose `user_version` exceeds the binary's newest known
migration is rejected with an explicit error rather than opened. The current schema carries no
`user_version` and is therefore version zero: it is adopted in place without data loss, so existing
local deployments and the initial vertical slice keep opening correctly.

## Consequences

Administrative retries and fixture re-seeding become safe by construction, and replacement semantics
(`INSERT OR REPLACE`, `INSERT OR IGNORE`) disappear from the write paths. Correcting published
content now costs a new immutable key: operators mint a new corpus or profile version instead of
editing, which is the intended audit property but removes any quiet fix. Migrations add a small
amount of startup-path complexity in exchange for safe forward evolution and a clear refusal on
databases written by a newer binary.
