use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State, WebSocketUpgrade, ws::Message},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    AppState, Error, InstanceState,
    schemas::{
        Capabilities, ExecutionRequest, ExecutionResponse, ExecutionResponses, NegotiationResponse,
    },
};

pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "hello": "world"
        })),
    )
}

#[axum::debug_handler]
pub async fn live(ws: WebSocketUpgrade, State(_app_state): State<AppState>) -> Response {
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

                    let _ = tx
                        .lock()
                        .await
                        .send(Message::Text(
                            serde_json::to_string(&NegotiationResponse::Success {
                                interface: instance.dts.clone(),
                                session_id: None,
                            })
                            .unwrap()
                            .into(),
                        ))
                        .await;

                    instance_state = Some(instance);
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

pub async fn session_create(
    State(AppState { sessions, .. }): State<AppState>,
    Json(capabilities): Json<Capabilities>,
) -> impl IntoResponse {
    match async {
        let id = Uuid::new_v4().to_string();

        let instance = InstanceState::new(&capabilities).await?;
        let interface = instance.dts.clone();
        sessions.write().await.put(id.clone(), Arc::new(instance));

        Ok::<_, Error>((interface, id))
    }
    .await
    {
        Ok((interface, id)) => (
            StatusCode::OK,
            Json(NegotiationResponse::Success {
                interface,
                session_id: Some(id),
            }),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(NegotiationResponse::Error {
                message: e.to_string(),
            }),
        ),
    }
}

pub async fn session_get(
    State(AppState { sessions, .. }): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let Some(instance) = sessions.write().await.get(&session_id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(NegotiationResponse::Error {
                message: Error::SessionNotFound.to_string(),
            }),
        );
    };

    (
        StatusCode::OK,
        Json(NegotiationResponse::Success {
            interface: instance.dts.clone(),
            session_id: None,
        }),
    )
}

pub async fn session_execute(
    State(AppState { sessions, .. }): State<AppState>,
    Path(session_id): Path<String>,
    Json(ExecutionRequest { code }): Json<ExecutionRequest>,
) -> impl IntoResponse {
    let mut responses = Vec::new();
    if let Err(e) = async {
        let Some(instance) = sessions.write().await.get(&session_id).cloned() else {
            return Err(Error::SessionNotFound);
        };

        instance.sandbox.execute(&code).await?;

        responses.extend(
            instance
                .sandbox
                .console_rx
                .drain()
                .map(|message| ExecutionResponse::Console { message }),
        );

        Ok::<_, Error>(())
    }
    .await
    {
        responses.push(ExecutionResponse::Error {
            message: e.to_string(),
        });
    }

    (StatusCode::OK, Json(ExecutionResponses { responses }))
}

pub async fn session_delete(
    State(AppState { sessions, .. }): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    sessions.write().await.pop(&session_id);

    StatusCode::OK
}
