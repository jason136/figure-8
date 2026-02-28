use axum::{
    Json,
    extract::{State, WebSocketUpgrade, ws::Message},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{
    Error, InstanceState,
    schemas::{ExecutionRequest, InstanceConfig, StreamResponse, SuccessResponse},
};

#[derive(Clone)]
pub struct AppState {}

pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "hello": "world"
        })),
    )
}

#[axum::debug_handler]
pub async fn stream(ws: WebSocketUpgrade, State(_app_state): State<AppState>) -> Response {
    ws.on_upgrade(move |mut socket| async move {
        let mut instance_state: Option<InstanceState> = None;

        while let Some(Ok(message)) = socket.recv().await {
            let msg_bytes = match message {
                Message::Text(ref text) => text.as_bytes(),
                Message::Binary(ref binary) => binary.as_ref(),
                Message::Ping(ping) => {
                    let _ = socket.send(Message::Pong(ping)).await;
                    continue;
                }
                Message::Pong(pong) => {
                    let _ = socket.send(Message::Ping(pong)).await;
                    continue;
                }
                Message::Close(_) => break,
            };

            let Ok(response) = async {
                let success = if let Some(instance_state) = &mut instance_state {
                    let ExecutionRequest { code } = serde_json::from_slice(msg_bytes)?;

                    let output = instance_state.sandbox.execute(&code).await?;

                    Ok::<_, Error>(SuccessResponse::Execution { output })
                } else {
                    let InstanceConfig { capabilities } = serde_json::from_slice(msg_bytes)?;

                    instance_state = Some(InstanceState::new(&capabilities)?);

                    Ok::<_, Error>(SuccessResponse::Negotiation { capabilities })
                }?;

                Ok::<_, Error>(serde_json::to_string(&StreamResponse::Success(success)))
            }
            .await
            .unwrap_or_else(|e| {
                serde_json::to_string(&StreamResponse::Error {
                    message: e.to_string(),
                })
            }) else {
                continue;
            };

            let _ = socket.send(Message::Text(response.into())).await;
        }
    })
}
