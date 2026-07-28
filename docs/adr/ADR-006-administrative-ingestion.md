# ADR-006: Administrative ingestion and publication

## Status

Accepted.

## Decision

PDF import is a separate administrative CLI. It records usage rights, inspects and hashes the file,
runs a configured scanner, extracts text in a bounded worker, creates a review bundle and requires
human approval of that exact bundle before publishing an immutable corpus version.

Image-only or ambiguous PDFs remain quarantined; OCR is deferred.

## Consequences

Agents cannot import or approve sources. Real publication requires operational scanner and reviewer
configuration, while synthetic fixtures use a dedicated deterministic test scanner.
