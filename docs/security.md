# Security model

Primary threats are cross-tenant object access, licensed-text leakage, prompt injection embedded in
documents, parser exploitation, forged continuations and confused-deputy authorization.

Controls include mandatory tenant-scoped repositories, composite tenant keys, opaque signed cursors,
strict layer typing, immutable corpus versions, resource limits around PDF extraction and metadata-
only audit logs. Authorization errors do not confirm whether another tenant's object exists.

The local server makes no outbound request for indexing, tokenization, telemetry or embeddings.
Production publication requires a configured security scanner; the deterministic fixture scanner
exists only for copyright-safe test data.
