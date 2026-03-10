use figure_8::sandbox::inspector::ConsoleMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct BrowserCapability {}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpCapability {
    pub server: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub browser: Option<BrowserCapability>,
    pub mcp: Vec<McpCapability>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result")]
#[serde(rename_all = "snake_case")]
pub enum NegotiationResponse {
    Success { interface: String },
    Error { message: String },
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
