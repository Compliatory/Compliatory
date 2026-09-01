#![forbid(unsafe_code)]

//! MCP 2025-11-25 adapter for the transport-independent regulatory service.

use compliatory_application::{
    AuthContext, BuildWorkPacketInput, CheckProfileCoverageInput, CheckProfileCoverageOutput,
    ExpandWorkPacketInput, RegulatoryService, SearchReferencesInput, SearchReferencesOutput,
    ValidateCitationsInput, ValidateCitationsOutput,
};
use compliatory_core::{DomainError, ErrorCode as DomainErrorCode, WorkPacket};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::server::wrapper::{Json, Parameters},
    model::{
        Implementation, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
        PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResult, ResourceContents,
        ResourceTemplate, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde_json::json;

#[derive(Clone)]
pub struct CompliatoryMcpServer {
    service: RegulatoryService,
    auth: AuthContext,
}

impl CompliatoryMcpServer {
    #[must_use]
    pub const fn new(service: RegulatoryService, auth: AuthContext) -> Self {
        Self { service, auth }
    }

    #[must_use]
    pub fn auth_context(&self) -> &AuthContext {
        &self.auth
    }
}

#[tool_router]
impl CompliatoryMcpServer {
    #[tool(
        name = "search_references",
        description = "Search authorised catalog, guidance and normative regulatory references"
    )]
    fn search_references(
        &self,
        Parameters(input): Parameters<SearchReferencesInput>,
    ) -> Result<Json<SearchReferencesOutput>, McpError> {
        self.service
            .search_references(&self.auth, &input)
            .map(Json)
            .map_err(domain_error)
    }

    #[tool(
        name = "build_work_packet",
        description = "Build a deterministic, token-bounded work packet for an explicit phase and role"
    )]
    fn build_work_packet(
        &self,
        Parameters(input): Parameters<BuildWorkPacketInput>,
    ) -> Result<Json<WorkPacket>, McpError> {
        self.service
            .build_work_packet(&self.auth, &input)
            .map(Json)
            .map_err(domain_error)
    }

    #[tool(
        name = "expand_work_packet",
        description = "Expand one level of explicit relations without changing workflow phase"
    )]
    fn expand_work_packet(
        &self,
        Parameters(input): Parameters<ExpandWorkPacketInput>,
    ) -> Result<Json<WorkPacket>, McpError> {
        self.service
            .expand_work_packet(&self.auth, &input)
            .map(Json)
            .map_err(domain_error)
    }

    #[tool(
        name = "validate_citations",
        description = "Mechanically validate exact citations against the approved tenant corpus"
    )]
    fn validate_citations(
        &self,
        Parameters(input): Parameters<ValidateCitationsInput>,
    ) -> Result<Json<ValidateCitationsOutput>, McpError> {
        self.service
            .validate_citations(&self.auth, &input)
            .map(Json)
            .map_err(domain_error)
    }

    #[tool(
        name = "check_profile_coverage",
        description = "Check reference coverage of an immutable regulatory profile without deciding compliance"
    )]
    fn check_profile_coverage(
        &self,
        Parameters(input): Parameters<CheckProfileCoverageInput>,
    ) -> Result<Json<CheckProfileCoverageOutput>, McpError> {
        self.service
            .check_profile_coverage(&self.auth, &input)
            .map(Json)
            .map_err(domain_error)
    }
}

#[tool_handler(
    name = "compliatory",
    version = "0.1.0",
    instructions = "Bounded, versioned and citable regulatory reference access. Guidance is non-normative."
)]
impl ServerHandler for CompliatoryMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(
            Implementation::new("compliatory", env!("CARGO_PKG_VERSION"))
                .with_title("Compliatory Regulatory Reference Service")
                .with_description("Bounded, versioned and citable regulatory access"),
        )
        .with_instructions(
            "Bounded, versioned regulatory access. Guidance and synthetic fixtures are non-normative.",
        )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> {
        std::future::ready(Ok(ListToolsResult {
            tools: Self::tool_router().list_all(),
            meta: None,
            next_cursor: None,
        }))
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> {
        // Dynamic tenant-scoped resources are deliberately advertised as templates. Packet URIs
        // returned by build/expand can be read directly without revealing another tenant's list.
        std::future::ready(Ok(ListResourcesResult::with_all_items(vec![])))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, McpError>> {
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new(
                "reg://standards/{standard_id}/{edition}/{language}/clauses/{locator}",
                "regulatory-clause",
            )
            .with_title("Regulatory clause")
            .with_description("Catalog node and authorised fragments for a canonical locator")
            .with_mime_type("application/json"),
            ResourceTemplate::new(
                "reg://profiles/{profile_id}/versions/{version}",
                "regulatory-profile",
            )
            .with_title("Immutable applicability profile")
            .with_mime_type("application/json"),
            ResourceTemplate::new("reg://packets/{packet_id}", "regulatory-work-packet")
                .with_title("Immutable work packet")
                .with_mime_type("application/json"),
        ])))
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, McpError>> {
        let result = self
            .service
            .read_resource(&self.auth, &request.uri)
            .map_err(|error| {
                if matches!(error.code, DomainErrorCode::UnknownReference) {
                    McpError::resource_not_found(
                        "resource not found",
                        Some(json!({"code": error.code.as_str()})),
                    )
                } else {
                    domain_error(error)
                }
            })
            .and_then(|resource| {
                serde_json::to_string(&resource.value)
                    .map_err(|error| McpError::internal_error(error.to_string(), None))
                    .map(|text| {
                        ReadResourceResult::new(vec![
                            ResourceContents::text(text, resource.uri)
                                .with_mime_type(resource.mime_type),
                        ])
                    })
            });
        std::future::ready(result)
    }
}

pub async fn run_stdio(server: CompliatoryMcpServer) -> anyhow::Result<()> {
    let running = server.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

fn domain_error(error: DomainError) -> McpError {
    let data = json!({
        "code": error.code.as_str(),
        "message": error.message,
    });
    match error.code {
        DomainErrorCode::Internal => McpError::internal_error("internal service error", Some(data)),
        _ => McpError::invalid_params("regulatory request rejected", Some(data)),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use compliatory_application::RegulatoryService;
    use compliatory_sqlite::SqliteRepository;

    use super::*;

    #[test]
    fn stdio_identity_is_injected_once() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        let service = RegulatoryService::new(repository, [9_u8; 32]).unwrap();
        let server = CompliatoryMcpServer::new(
            service,
            AuthContext::local_service("tenant-a", "service:test"),
        );
        assert_eq!(server.auth_context().tenant_id, "tenant-a");
    }

    #[test]
    fn server_exposes_exactly_the_five_regulatory_tools() {
        let repository = Arc::new(SqliteRepository::open_in_memory().unwrap());
        let service = RegulatoryService::new(repository, [9_u8; 32]).unwrap();
        let server = CompliatoryMcpServer::new(
            service,
            AuthContext::local_service("tenant-a", "service:test"),
        );
        let names = [
            "search_references",
            "build_work_packet",
            "expand_work_packet",
            "validate_citations",
            "check_profile_coverage",
        ];
        for name in names {
            let tool = ServerHandler::get_tool(&server, name)
                .unwrap_or_else(|| panic!("missing MCP tool {name}"));
            assert!(
                tool.output_schema.is_some(),
                "{name} needs an output schema"
            );
        }
        assert!(ServerHandler::get_tool(&server, "import_pdf").is_none());
    }
}
