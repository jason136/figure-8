use std::sync::Arc;

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    service::{Peer, RoleClient, RunningService},
    transport::StreamableHttpClientTransport,
};

use crate::{
    FnDefError, JsApi, TsType, fn_def_async,
    sandbox::{fn_def::FnDef, interface::Interface, marshall::json_schema_ts_type},
};

#[derive(Clone)]
pub struct Mcp {
    peer: Peer<RoleClient>,
    tools: Arc<Vec<rmcp::model::Tool>>,
    _service: Arc<RunningService<RoleClient, ()>>,
}

impl Interface for Mcp {
    fn extend_api(&self, js_api: &mut JsApi) -> Result<(), FnDefError> {
        js_api.extend_fn_defs(self.default_tools()?)?;

        Ok(())
    }
}

impl Mcp {
    pub async fn new(server: &str) -> Result<Self, FnDefError> {
        let service = ().serve(StreamableHttpClientTransport::from_uri(server)).await?;
        let peer = service.peer().clone();
        let tools = service.list_all_tools().await?;

        Ok(Mcp {
            peer,
            tools: Arc::new(tools),
            _service: Arc::new(service),
        })
    }

    pub fn default_tools(&self) -> Result<Vec<FnDef>, FnDefError> {
        self.tools
            .iter()
            .map(|tool| {
                let description = tool
                    .description
                    .as_ref()
                    .map(|d| d.to_string())
                    .unwrap_or_default();

                let param_type = json_schema_ts_type(&tool.input_schema);
                let TsType::Object(tool_params) = param_type else {
                    return Err(FnDefError::McpParamType(param_type));
                };

                let ret = tool
                    .output_schema
                    .as_deref()
                    .map(json_schema_ts_type)
                    .unwrap_or(TsType::Unknown);

                let peer = self.peer.clone();
                let tool_name = tool.name.to_string();

                let mut fn_def =
                    fn_def_async!("mcp", "", |input: serde_json::Value| -> serde_json::Value {
                        let peer_clone = peer.clone();
                        let tool_name_clone = tool_name.clone();
                        async move {
                            let arguments = input.as_object().filter(|m| !m.is_empty()).cloned();
                            let result = peer_clone
                                .call_tool(CallToolRequestParams {
                                    meta: None,
                                    name: tool_name_clone.into(),
                                    arguments,
                                    task: None,
                                })
                                .await?;
                            Ok::<_, FnDefError>(serde_json::to_value(&result).unwrap_or_default())
                        }
                    });

                fn_def.name = format!("mcp.{}", tool.name);
                fn_def.description = description;
                fn_def.params = vec![("params".to_string(), TsType::Object(tool_params))];
                fn_def.ret = TsType::Promise(Box::new(ret));

                Ok(fn_def)
            })
            .collect()
    }
}
