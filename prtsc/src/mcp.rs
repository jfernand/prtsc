//! An MCP server, served over stdio, exposing the screenshot-portal capture
//! action as a single tool.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::transport::stdio;
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};

use crate::capture;

#[derive(Clone)]
struct CaptureServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl CaptureServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Initiate a screen capture via the system's screenshot picker and return the saved file's location"
    )]
    async fn capture(&self) -> Result<CallToolResult, McpError> {
        match capture::capture().await {
            Ok(uri) => Ok(CallToolResult::success(vec![Content::text(uri)])),
            Err(err) => Ok(CallToolResult::error(vec![Content::text(err)])),
        }
    }
}

#[tool_handler]
impl ServerHandler for CaptureServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Exposes a single tool to initiate a screen capture via the system's \
                 screenshot portal."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

/// Runs the MCP server over stdio until the client disconnects.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let service = CaptureServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
