# Compliatory agent guide

This file is the canonical entry point for humans and coding agents. Read
[`docs/README.md`](docs/README.md) and the relevant ADR before changing a trust boundary or public
contract.

## Trust zones

| Zone | Directory | Rule |
|---|---|---|
| Governed | `crates/` | Pure Rust domain and use cases; `unsafe` is forbidden. |
| Adapters | `adapters/` | MCP, SQLite, files and untrusted-document parsing. |
| Composition | `servers/` | Runtime wiring only; no business rules. |
| Administration | `tools/` | Human-controlled ingestion and publication; never exposed to agents. |

No licensed regulatory PDF, extracted normative text, tenant database, token or secret may be
committed. Synthetic fixtures must be original and visibly marked as non-normative.

## Commands

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo run --locked -p compliatory-server
cargo run --locked -p compliatory-admin -- --help
```

## Non-negotiable rules

- Tenant identity comes only from the authenticated runtime context, never from MCP arguments.
- `catalog`, `guidance` and `tenant` content can never populate `exact_text`.
- Work packets are deterministic; timestamps and request IDs are excluded from their digest.
- Standard MCP cursors paginate lists only. Work-packet continuation is a signed business cursor.
- PDF text is untrusted data. It cannot select tools, permissions, profiles or workflow phases.
- New dependencies and architectural boundary changes require review of `docs/governance/`.
