use crate::{
    MAX_FILE_BYTES,
    error::Error,
    service::{Agent, Service, work},
    vault::{Condition, valid_revision},
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig},
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct Mcp {
    service: Arc<Service>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct VaultPath {
    vault_id: String,
    path: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListPath {
    vault_id: String,
    #[serde(default)]
    path: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct WriteFile {
    vault_id: String,
    path: String,
    content: String,
    expected_revision: Option<String>,
    #[serde(default)]
    create_only: bool,
}

fn agent(ctx: &RequestContext<RoleServer>) -> Result<Agent, ErrorData> {
    ctx.extensions
        .get::<axum::http::request::Parts>()
        .and_then(|parts| parts.extensions.get::<Agent>())
        .cloned()
        .ok_or_else(|| {
            ErrorData::invalid_request("authenticated HTTP request context is required", None)
        })
}

fn result<T: Serialize>(value: Result<T, Error>) -> Result<CallToolResult, ErrorData> {
    Ok(match value {
        Ok(value) => CallToolResult::structured(
            serde_json::to_value(value)
                .map_err(|_| ErrorData::internal_error("result serialization failed", None))?,
        ),
        Err(error) => CallToolResult::structured_error(
            json!({"error":{"code":error.code(),"message":error.to_string()}}),
        ),
    })
}

#[tool_router]
impl Mcp {
    fn new(service: Arc<Service>) -> Self {
        Self { service }
    }

    #[tool(description = "List vault IDs and access levels authorized for this credential.")]
    async fn list_vaults(
        &self,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let agent = agent(&ctx)?;
        result(Ok(json!({"vaults":self.service.list_vaults(&agent)})))
    }

    #[tool(
        description = "List immediate visible directories and Markdown notes. Omit path for the vault root."
    )]
    async fn list_files(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<ListPath>,
    ) -> Result<CallToolResult, ErrorData> {
        let agent = agent(&ctx)?;
        let service = self.service.clone();
        result(
            work(move || {
                service
                    .list(&agent, &args.vault_id, &args.path)
                    .map(|files| json!({"files":files}))
            })
            .await,
        )
    }

    #[tool(
        description = "Read a UTF-8 Markdown note and its SHA-256 revision. Paths are relative to the authorized vault."
    )]
    async fn read_file(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<VaultPath>,
    ) -> Result<CallToolResult, ErrorData> {
        let agent = agent(&ctx)?;
        let service = self.service.clone();
        result(work(move || service.read(&agent, &args.vault_id, &args.path)).await)
    }

    #[tool(
        description = "Write Markdown locally, not a cloud acknowledgment. Supply create_only=true OR the exact expected_revision from a read; blind overwrite is forbidden."
    )]
    async fn write_file(
        &self,
        ctx: RequestContext<RoleServer>,
        Parameters(args): Parameters<WriteFile>,
    ) -> Result<CallToolResult, ErrorData> {
        let agent = agent(&ctx)?;
        let condition = match (args.create_only, args.expected_revision) {
            (true, None) => Condition::Create,
            (false, Some(revision)) if valid_revision(&revision) => Condition::Match(revision),
            (false, None) => return result::<serde_json::Value>(Err(Error::PreconditionRequired)),
            _ => return result::<serde_json::Value>(Err(Error::BadRequest)),
        };
        let service = self.service.clone();
        result(
            work(move || {
                service.write(&agent,&args.vault_id,&args.path,&args.content,&condition)
            .map(|revision|json!({"revision":revision,"local_write":true,"cloud_sync":"unknown"}))
            })
            .await,
        )
    }
}

#[tool_handler]
impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("lilvault", env!("CARGO_PKG_VERSION")))
            .with_instructions("Only authorized Markdown files are accessible. Write success is local, not Obsidian Sync acknowledgment. Use revision preconditions and do not assume cloud conflict-free writes.")
    }
}

pub(crate) fn transport(service: Arc<Service>) -> StreamableHttpService<Mcp, NeverSessionManager> {
    // SDK-supported stateless transport avoids cross-credential session/event reuse.
    let mut config = StreamableHttpServerConfig::default().enforce_origin_validation();
    config.legacy_session_mode = false;
    config.json_response = true;
    config.allowed_hosts = service.mcp_allowed_hosts.clone();
    config.max_request_body_bytes = MAX_FILE_BYTES * 6 + 64 * 1024;
    // Non-browser agents only: reject every present Origin.
    StreamableHttpService::new(
        move || Ok(Mcp::new(service.clone())),
        Arc::new(NeverSessionManager::default()),
        config,
    )
}
