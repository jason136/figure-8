use std::sync::Arc;

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    service::{Peer, RoleClient, RunningService},
    transport::StreamableHttpClientTransport,
};
use tokio::sync::oneshot;

use crate::{
    FromV8, ToolError, TsType,
    sandbox::{
        marshall::json_schema_ts_type,
        tool::{DeferredValue, PendingPromise, ToolDef, ToolHandler},
    },
};

#[derive(Clone)]
pub struct Mcp {
    peer: Peer<RoleClient>,
    tools: Arc<Vec<rmcp::model::Tool>>,
    _service: Arc<RunningService<RoleClient, ()>>,
}

impl Mcp {
    pub async fn new(server: &str) -> Result<Self, ToolError> {
        let service = ().serve(StreamableHttpClientTransport::from_uri(server)).await?;
        let peer = service.peer().clone();
        let tools = service.list_all_tools().await?;

        Ok(Mcp {
            peer,
            tools: Arc::new(tools),
            _service: Arc::new(service),
        })
    }

    pub fn default_tools(&self) -> Result<Vec<ToolDef>, ToolError> {
        self.tools
            .iter()
            .map(|tool| {
                let name = tool.name.to_string();
                let description = tool
                    .description
                    .as_ref()
                    .map(|d| d.to_string())
                    .unwrap_or_default();

                let param_type = json_schema_ts_type(&tool.input_schema);
                let TsType::Object(params) = param_type else {
                    return Err(ToolError::McpParamType(param_type));
                };
                let param_names = params
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>();

                let ret = tool
                    .output_schema
                    .as_deref()
                    .map(json_schema_ts_type)
                    .unwrap_or(TsType::Unknown);

                let peer = self.peer.clone();
                let handler = ToolHandler::Async(Box::new(move |scope, args, mut rv, pending| {
                    let arguments = param_names
                        .iter()
                        .enumerate()
                        .filter_map(|(i, name)| {
                            serde_json::Value::from_v8(scope, args.get(i as i32))
                                .ok()
                                .map(|val| (name.clone(), val))
                        })
                        .collect::<serde_json::Map<_, _>>();

                    let resolver = v8::PromiseResolver::new(scope).unwrap();
                    rv.set(resolver.get_promise(scope).into());
                    let resolver = v8::Global::new(scope, resolver);

                    let (tx, rx) = oneshot::channel();
                    pending.send(PendingPromise { resolver, rx }).unwrap();

                    let peer_clone = peer.clone();
                    let name_clone = name.clone().into();
                    tokio::spawn(async move {
                        let result = peer_clone
                            .call_tool(CallToolRequestParams {
                                meta: None,
                                name: name_clone,
                                arguments: (!arguments.is_empty()).then_some(arguments),
                                task: None,
                            })
                            .await;

                        let tool_result = result
                            .map(|call_result| {
                                Box::new(serde_json::to_value(&call_result).unwrap_or_default())
                                    as Box<dyn DeferredValue>
                            })
                            .map_err(|e| e.to_string());

                        let _ = tx.send(tool_result);
                    });
                }));

                Ok(ToolDef {
                    name: format!("mcp.{}", tool.name),
                    description,
                    params,
                    ret,
                    handler,
                })
            })
            .collect()
    }
}
