# MCP contract

The server targets MCP revision `2025-11-25`. It exposes resources and tools, but no prompts,
sampling or task-augmented tools.

## Resources

- `reg://standards/{standard_id}/{edition}/{language}/clauses/{locator}`
- `reg://profiles/{profile_id}/versions/{version}`
- `reg://packets/{packet_id}`

Tenant identity never appears in a URI. Resource reads re-check entitlement and approval.

## Tools

- `search_references`
- `build_work_packet`
- `expand_work_packet`
- `validate_citations`
- `check_profile_coverage`

Every tool returns structured JSON. Standard MCP cursors are used only for lists. Packet
continuations are signed business cursors bound to the authenticated tenant, operation inputs and
corpus versions.

## Authorization context

Every call receives an internal `AuthContext` containing `tenant_id`, `subject_id` and a fixed set of
permissions. STDIO loads this context from process configuration. A future Streamable HTTP adapter
will derive it from an audience-bound OAuth access token.

Administrative import, approval and entitlement changes are not MCP tools.
