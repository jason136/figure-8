use figure_8::sandbox::inspector::ConsoleMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct FsCapability {}

#[derive(Debug, Serialize, Deserialize)]
pub struct FetchCapability {}

#[derive(Debug, Serialize, Deserialize)]
pub struct BrowserCapability {}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpCapability {
    pub server: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub fs: Option<FsCapability>,
    pub fetch: Option<FetchCapability>,
    pub browser: Option<BrowserCapability>,
    pub mcp: Vec<McpCapability>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result")]
#[serde(rename_all = "snake_case")]
pub enum NegotiationResponse {
    Success {
        interface: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionRequest {
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "channel")]
#[serde(rename_all = "snake_case")]
pub enum ExecutionResponse {
    Console { message: ConsoleMessage },
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionResponses {
    pub responses: Vec<ExecutionResponse>,
}
