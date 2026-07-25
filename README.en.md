# Compliatory

🇫🇷 [Version française](README.md)

Compliatory is a specialised MCP service that exposes bounded, versioned and verifiable regulatory
references. It keeps catalog metadata, non-normative guidance, exact licensed fragments and
tenant-authored rules visibly separate.

The server does not decide compliance, final applicability or workflow sequencing. It exposes
citable resources, builds deterministic work packets and mechanically validates citations and
profile coverage.

The initial vertical slice provides an MCP STDIO server, SQLite and filesystem persistence,
copyright-safe synthetic fixtures and an administrative text-PDF ingestion workflow.

```bash
cargo build --locked --workspace
cargo test --locked --workspace
cargo run --locked -p compliatory-admin -- --data-dir .compliatory seed-fixtures
cargo run --locked -p compliatory-server
```

See the [documentation](docs/README.md) and
[architecture decision records](docs/adr/README.md).
The [operations guide](docs/operations.md) covers local identity and the approved import lifecycle.

**License:** FSL-1.1-ALv2.
