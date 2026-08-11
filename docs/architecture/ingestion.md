# Administrative ingestion

The ingestion state machine is:

```text
received -> inspected -> extracted -> awaiting_review -> approved -> published
                    \-> quarantined
```

An import manifest declares standard, edition, amendments, language, usage-right statement and
expected structure. The importer computes a source digest, checks the PDF envelope, invokes the
configured security scanner, extracts its text in a worker process and produces a review bundle.

The reviewer approves the digest of the complete bundle. Publication creates a new immutable corpus
version; fixes are never applied invisibly. Image-only PDFs remain quarantined until a separately
approved OCR adapter is introduced.

The transition to `published` commits fragments, the corpus version row and the ingestion state in
one transaction, and replaying it returns the existing published record. See
[ADR-007](../adr/ADR-007-immutable-writes-and-schema-evolution.md).
