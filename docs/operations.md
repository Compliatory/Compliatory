# Operations

This guide describes the local STDIO deployment. A hosted transport must supply an authenticated
identity and enforce the same application permissions; tenant identity must never come from an MCP
tool argument.

## Initialise a local tenant

```bash
export COMPLIATORY_DATA_DIR=.compliatory
export COMPLIATORY_TENANT=local
cargo run --locked -p compliatory-admin -- seed-fixtures
```

The fixtures contain original, non-normative material only. Re-running the command is idempotent.
Runtime data is excluded from version control.

## Start the MCP server

```bash
export COMPLIATORY_SUBJECT=service:local-stdio
export COMPLIATORY_CURSOR_KEY='replace-with-at-least-32-random-bytes'
cargo run --locked -p compliatory-server
```

Logs are written to standard error so that standard output remains a valid MCP JSON-RPC stream.
The built-in cursor key fallback is intended only for single-user local development. Production
operators must inject and rotate a secret of at least 32 bytes, retaining old keys while their
issued continuation cursors remain valid.

## Import a licensed text PDF

The operator must first create a manifest like
[`fixtures/manifests/import-example.json`](../fixtures/manifests/import-example.json), then invoke
an approved security scanner executable:

```bash
cargo run --locked -p compliatory-admin -- ingest \
  --manifest fixtures/manifests/import-example.json \
  --pdf /authorised/input/document.pdf \
  --scanner-command /operator/approved/pdf-scanner
```

The scanner receives the PDF path as its sole argument and must return zero only for an accepted
file. Compliatory then starts a separate extraction worker with a minimal environment. The source
and extracted review bundle remain under the tenant's `quarantine/` directory.

Review the exact source against the generated bundle:

```bash
cargo run --locked -p compliatory-admin -- review --ingestion-id INGESTION_ID
cargo run --locked -p compliatory-admin -- approve \
  --ingestion-id INGESTION_ID \
  --review-digest REVIEW_DIGEST \
  --approver operator@example.invalid
cargo run --locked -p compliatory-admin -- publish --ingestion-id INGESTION_ID
```

Approval is bound to the printed review digest. Any extraction correction requires a new import
and approval. Publication creates an immutable corpus version; it does not modify an existing
version.

## Failure handling

- A scanner rejection or extraction failure leaves the source quarantined and unpublished.
- An image-only PDF is rejected because OCR is not enabled in this vertical slice.
- A mismatched review digest is rejected without changing approval state.
- Backup and restore the complete data directory as one unit: SQLite database, tenant corpus,
  quarantine records and packet cache must remain consistent.
- Never copy one tenant's directory or database rows into another tenant.
