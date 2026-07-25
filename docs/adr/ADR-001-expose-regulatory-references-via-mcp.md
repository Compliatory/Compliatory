# ADR-001: Expose regulatory references through MCP

## Status

Accepted — 2026-07-24.

## Provenance

This is Compliatory's founding decision. It is a faithful, renumbered adoption of LaFerme
ADR-0007, “Exposer les référentiels réglementaires via MCP”. The
[French source](sources/LaFerme-ADR-0007.fr.md) is retained in this repository as provenance, with
only its relative architecture link adjusted to this repository; this record states the resulting
project boundary.

## Context

LaFerme's scoping, evaluation, coding, testing and independent-review agents need exact regulatory
references without receiving whole standards in every prompt. Full normative documents are
licensed and cannot be reconstructed from model memory or explanatory prose.

Agents must discover editions and profiles, retrieve precisely identified units, receive only the
definitions and relations required for their role, cite authorised text, and mechanically validate
citations and profile coverage.

## Decision

Compliatory provides a bounded, versioned and citable regulatory data plane over MCP. LaFerme keeps
workflow state and selects profile, phase, role, objective, context budget and accessible project
evidence. Compliatory never decides final applicability, compliance or workflow sequencing.

Content is visibly separated into `catalog`, `guidance`, `normative` and `tenant` layers. Exact text
is served only from an approved, licensed tenant corpus. Without one, the service returns
`full_text_unavailable` and never invents normative wording.

Work packets are deterministic and contain authorised fragments, separate guidance, definitions,
relations, versions, source digests, budget accounting, omissions, continuation and a complete
digest. MCP pagination remains reserved for lists.

The pilot contract covers IEC 62304:2006+A1:2015, IEC 81001-5-1:2021 and DO-178C/ED-12C, with
DO-178B/ED-12B represented as a distinct edition. Repository fixtures model these families but
contain no protected text.

The same logical data plane supports dedicated and isolated multi-tenant deployments. Tenant
identity is derived from authentication, never supplied by a model.

PDF import is a separate administrative workflow with rights declaration, hashing, security
inspection, isolated extraction, structural matching, human review and immutable publication.
Prompts and MCP Tasks are not part of the first version.

## Consequences

Agent context is smaller and findings can be replayed against a corpus version, at the cost of
additional document calls and substantial human curation. MCP standardises access but does not make
the service an autonomous certifier.

## Reconsider when

Reconsider this decision if MCP cannot preserve stable URIs and deterministic results, licensing
prevents fragment delivery, physical separation makes the shared logical model unsuitable, or
approved normative units routinely exceed the ordinary 8,192-token limit.
