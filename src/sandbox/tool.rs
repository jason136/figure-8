use std::cell::RefCell;
use std::collections::VecDeque;

use super::marshal::IntoV8;

pub type SyncCallback = Box<
    dyn Fn(&mut v8::PinScope<'_, '_>, v8::FunctionCallbackArguments, v8::ReturnValue) + Send + Sync,
>;

pub type AsyncCallback = Box<
    dyn Fn(&mut v8::PinScope<'_, '_>, v8::FunctionCallbackArguments, v8::ReturnValue, &Pending)
        + Send
        + Sync,
>;

pub enum ToolHandler {
    Sync(SyncCallback),
    Async(AsyncCallback),
}

pub trait DeferredValue: Send + 'static {
    fn to_v8<'s>(self: Box<Self>, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value>;
}

impl<T: IntoV8 + Send> DeferredValue for T {
    fn to_v8<'s>(self: Box<Self>, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        (*self).into_v8(scope)
    }
}

pub type ToolResult = Result<Box<dyn DeferredValue>, String>;

pub struct PendingPromise {
    pub resolver: v8::Global<v8::PromiseResolver>,
    pub rx: tokio::sync::oneshot::Receiver<ToolResult>,
}

pub type Pending = RefCell<VecDeque<PendingPromise>>;

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub ts_declaration: String,
    pub handler: ToolHandler,
}

impl ToolDef {
    pub fn new_sync(
        name: impl Into<String>,
        description: impl Into<String>,
        ts_declaration: impl Into<String>,
        handler: SyncCallback,
    ) -> Self {
        ToolDef {
            name: name.into(),
            description: description.into(),
            ts_declaration: ts_declaration.into(),
            handler: ToolHandler::Sync(handler),
        }
    }

    pub fn new_async(
        name: impl Into<String>,
        description: impl Into<String>,
        ts_declaration: impl Into<String>,
        handler: AsyncCallback,
    ) -> Self {
        ToolDef {
            name: name.into(),
            description: description.into(),
            ts_declaration: ts_declaration.into(),
            handler: ToolHandler::Async(handler),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Custom(String),
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl ToolError {
    pub fn custom(msg: impl Into<String>) -> Self {
        ToolError::Custom(msg.into())
    }
}
