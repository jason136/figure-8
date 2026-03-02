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
#[serde(tag = "result")]
#[serde(rename_all = "snake_case")]
pub enum NegotiationResponse {
    Success { capabilities: Vec<Capabilities> },
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
    Stdout { message: String },
    Stderr { message: String },
    Error { message: String },
}
