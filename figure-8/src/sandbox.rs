use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Once;

use flume::{Receiver, Sender, unbounded};
use tokio::sync::oneshot;

use interface::Interface;
use tool::PendingQueue;

use crate::{ToolError, sandbox::inspector::Inspector};

pub mod inspector;
pub mod interface;
pub mod marshal;
pub mod tool;

fn ensure_v8() {
    static V8_INIT: Once = Once::new();

    V8_INIT.call_once(|| {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

pub struct SandboxCommand {
    pub code: String,
    pub reply: oneshot::Sender<Result<String, SandboxError>>,
}

pub struct Sandbox {
    command_tx: Sender<SandboxCommand>,
    pub stdout: Receiver<String>,
    pub stderr: Receiver<String>,
    _handle: std::thread::JoinHandle<()>,
}

impl Sandbox {
    pub fn new(interface: Interface) -> Result<Self, ToolError> {
        ensure_v8();

        let (inspector, stdout, stderr) = Inspector::new();

        let (command_tx, command_rx) = unbounded();
        let tokio_handle = tokio::runtime::Handle::current();

        let _handle = std::thread::spawn(move || {
            let _guard = tokio_handle.enter();
            spawn_isolate(interface, command_rx, inspector);
        });

        Ok(Sandbox {
            command_tx,
            stdout,
            stderr,
            _handle,
        })
    }

    pub async fn execute(&self, code: &str) -> Result<String, SandboxError> {
        let (tx, rx) = oneshot::channel();

        self.command_tx
            .send(SandboxCommand {
                code: code.to_string(),
                reply: tx,
            })
            .map_err(|_| SandboxError::InternalError("sandbox thread has exited".to_string()))?;

        rx.await
            .map_err(|_| SandboxError::InternalError("sandbox dropped reply channel".to_string()))?
    }
}

fn spawn_isolate(interface: Interface, command_rx: Receiver<SandboxCommand>, inspector: Inspector) {
    let injected_ptrs;
    {
        let mut isolate = v8::Isolate::new(Default::default());
        let pending = Box::new(RefCell::new(VecDeque::new()));
        let pending_ptr: *const PendingQueue = &*pending;

        let _inspector =
            v8::inspector::V8Inspector::create(&mut isolate, inspector.into_inspector_client());

        let context_global = {
            let scope = std::pin::pin!(v8::HandleScope::new(&mut isolate));
            let scope = &mut scope.init();
            let context = v8::Context::new(scope, Default::default());

            let name_view = v8::inspector::StringView::from(&b"figure-8"[..]);
            _inspector.context_created(context, 1, name_view, v8::inspector::StringView::empty());

            let scope = &mut v8::ContextScope::new(scope, context);
            let global = context.global(scope);

            injected_ptrs = interface.inject(scope, global, pending_ptr);

            v8::Global::new(scope, context)
        };

        loop {
            let Ok(SandboxCommand { code, reply }) = command_rx.recv() else {
                break;
            };

            let result = execute_module(&mut isolate, &context_global, &code, &pending);
            let _ = reply.send(result);
        }
    };

    unsafe { interface::free_injected_data(injected_ptrs) };
}

fn execute_module(
    isolate: &mut v8::Isolate,
    context: &v8::Global<v8::Context>,
    code: &str,
    pending: &PendingQueue,
) -> Result<String, SandboxError> {
    let module_promise = {
        let scope = std::pin::pin!(v8::HandleScope::new(isolate));
        let scope = &mut scope.init();
        let ctx = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, ctx);

        let filename = v8::String::new(scope, "<module>").unwrap();
        let source_str = v8::String::new(scope, code).ok_or_else(|| {
            SandboxError::InternalError("failed to create source string".to_string())
        })?;

        let origin = v8::ScriptOrigin::new(
            scope,
            filename.into(),
            0,
            0,
            false,
            -1,
            None,
            false,
            false,
            true,
            None,
        );

        let mut source = v8::script_compiler::Source::new(source_str, Some(&origin));

        let tc = v8::TryCatch::new(scope);
        let tc = std::pin::pin!(tc);
        let tc = &mut tc.init();

        let Some(module) = v8::script_compiler::compile_module(tc, &mut source) else {
            return Err(SandboxError::JsError(
                tc.exception()
                    .map(|e| e.to_rust_string_lossy(tc))
                    .unwrap_or_else(|| "compilation error".to_string()),
            ));
        };

        if module.instantiate_module(tc, |_, _, _, _| None).is_none() {
            return Err(SandboxError::JsError(
                tc.exception()
                    .map(|e| e.to_rust_string_lossy(tc))
                    .unwrap_or_else(|| "module instantiation error".to_string()),
            ));
        }

        let Some(result) = module.evaluate(tc) else {
            return Err(SandboxError::JsError(
                tc.exception()
                    .map(|e| e.to_rust_string_lossy(tc))
                    .unwrap_or_else(|| "evaluation error".to_string()),
            ));
        };

        let promise: v8::Local<v8::Promise> = result.try_into().unwrap();
        v8::Global::new(tc, promise)
    };

    while let Some(p) = pending.borrow_mut().pop_front() {
        let result =
            p.rx.blocking_recv()
                .map_err(|_| SandboxError::InternalError("tool future dropped".to_string()))?;

        let scope = std::pin::pin!(v8::HandleScope::new(isolate));
        let scope = &mut scope.init();
        let ctx = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, ctx);

        let resolver = v8::Local::new(scope, &p.resolver);
        match result {
            Ok(value) => {
                let val = value.to_v8(scope);
                resolver.resolve(scope, val);
            }
            Err(msg) => {
                let err = v8::String::new(scope, &msg).unwrap();
                resolver.reject(scope, v8::Exception::error(scope, err));
            }
        }

        scope.perform_microtask_checkpoint();
    }

    let scope = std::pin::pin!(v8::HandleScope::new(isolate));
    let scope = &mut scope.init();
    let ctx = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, ctx);
    let promise = v8::Local::new(scope, module_promise);

    match promise.state() {
        v8::PromiseState::Fulfilled => Ok(promise.result(scope).to_rust_string_lossy(scope)),
        v8::PromiseState::Rejected => Err(SandboxError::JsError(
            promise.result(scope).to_rust_string_lossy(scope),
        )),
        v8::PromiseState::Pending => Err(SandboxError::InternalError(
            "module promise still pending".to_string(),
        )),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("javascript error: {0}")]
    JsError(String),

    #[error("internal error: {0}")]
    InternalError(String),
}
