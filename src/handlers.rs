use std::sync::Arc;

use axum::{
    Json,
    extract::{State, WebSocketUpgrade, ws::Message},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
    Error, InstanceState,
    schemas::{ExecutionRequest, ExecutionResponse, NegotiationResponse},
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
    ws.on_upgrade(move |socket| async move {
        let mut instance_state: Option<InstanceState> = None;
        let (tx, mut rx) = socket.split();
        let tx = Arc::new(Mutex::new(tx));

        while let Some(Ok(message)) = rx.next().await {
            let msg_bytes = match message {
                Message::Text(ref text) => text.as_bytes(),
                Message::Binary(ref binary) => binary.as_ref(),
                Message::Ping(ping) => {
                    let _ = tx.lock().await.send(Message::Pong(ping)).await;
                    continue;
                }
                Message::Pong(pong) => {
                    let _ = tx.lock().await.send(Message::Ping(pong)).await;
                    continue;
                }
                Message::Close(_) => break,
            };

            if let Err(e) = async {
                if let Some(instance_state) = &mut instance_state {
                    let ExecutionRequest { code } = serde_json::from_slice(msg_bytes)?;

                    instance_state.sandbox.execute(&code).await?;
                } else {
                    let capabilities = serde_json::from_slice(msg_bytes)?;

                    let instance = InstanceState::new(&capabilities).await?;

                    let console_rx = instance.sandbox.console_rx.clone();

                    let tx_clone = tx.clone();
                    tokio::spawn(async move {
                        loop {
                            let Ok(message) = console_rx.recv_async().await else {
                                return;
                            };

                            let message =
                                serde_json::to_string(&ExecutionResponse::Console { message })
                                    .unwrap();

                            let _ = tx_clone
                                .lock()
                                .await
                                .send(Message::Text(message.into()))
                                .await;
                        }
                    });

                    instance_state = Some(instance);

                    let _ = tx
                        .lock()
                        .await
                        .send(Message::Text(
                            serde_json::to_string(&NegotiationResponse::Success)
                                .unwrap()
                                .into(),
                        ))
                        .await;
                }

                Ok::<_, Error>(())
            }
            .await
            {
                let message = serde_json::to_string(&ExecutionResponse::Error {
                    message: e.to_string(),
                })
                .unwrap();

                let _ = tx.lock().await.send(Message::Text(message.into())).await;
            }
        }
    })
}
