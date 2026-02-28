use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum Capabilities {
    Browser,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub capabilities: Vec<Capabilities>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum SuccessResponse {
    Negotiation { capabilities: Vec<Capabilities> },
    Execution { output: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result")]
#[serde(rename_all = "snake_case")]
pub enum StreamResponse {
    Success(SuccessResponse),
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionRequest {
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionResponse {
    result: String,
}
