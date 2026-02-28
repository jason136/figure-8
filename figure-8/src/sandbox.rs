use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Once;

use flume::{Receiver, Sender, unbounded};
use tokio::sync::oneshot;

use interface::Interface;
use tool::PendingQueue;

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
    _handle: std::thread::JoinHandle<()>,
}

impl Sandbox {
    pub fn new(interface: Interface) -> Self {
        ensure_v8();

        let (command_tx, command_rx) = unbounded();
        let tokio_handle = tokio::runtime::Handle::current();

        let _handle = std::thread::spawn(move || {
            let _guard = tokio_handle.enter();
            spawn_isolate(interface, command_rx);
        });

        Sandbox {
            command_tx,
            _handle,
        }
    }

    pub async fn execute(&self, code: &str) -> Result<String, SandboxError> {
        let (tx, rx) = oneshot::channel();

        if self
            .command_tx
            .send(SandboxCommand {
                code: code.to_string(),
                reply: tx,
            })
            .is_err()
        {
            return Err(SandboxError::InternalError(
                "sandbox thread has exited".into(),
            ));
        }

        rx.await.unwrap_or(Err(SandboxError::InternalError(
            "sandbox dropped reply channel".into(),
        )))
    }
}

fn spawn_isolate(interface: Interface, command_rx: Receiver<SandboxCommand>) {
    let injected_ptrs;
    {
        let mut isolate = v8::Isolate::new(Default::default());
        let pending = Box::new(RefCell::new(VecDeque::new()));
        let pending_ptr: *const PendingQueue = &*pending;

        let context_global = {
            let scope = std::pin::pin!(v8::HandleScope::new(&mut isolate));
            let scope = &mut scope.init();
            let context = v8::Context::new(scope, Default::default());
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
    // Wrap in a module that exports the result of the last expression.
    let wrapped = format!("const __result = {code}; globalThis.__result = __result;");

    let module_promise = {
        let scope = std::pin::pin!(v8::HandleScope::new(isolate));
        let scope = &mut scope.init();
        let ctx = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, ctx);

        let name = v8::String::new(scope, "<sandbox>").unwrap();
        let Some(source_str) = v8::String::new(scope, &wrapped) else {
            return Err(SandboxError::InternalError(
                "failed to create V8 string".into(),
            ));
        };

        let origin = v8::ScriptOrigin::new(
            scope,
            name.into(),
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

        if module
            .instantiate_module(tc, stub_resolve_callback)
            .is_none()
        {
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

    loop {
        let p = pending.borrow_mut().pop_front();
        let Some(p) = p else { break };

        let Ok(result) = p.rx.blocking_recv() else {
            return Err(SandboxError::InternalError("tool future dropped".into()));
        };

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

    let promise = v8::Local::new(scope, &module_promise);
    match promise.state() {
        v8::PromiseState::Fulfilled => {
            let key = v8::String::new(scope, "__result").unwrap();
            let val = ctx.global(scope).get(scope, key.into()).unwrap();
            Ok(val.to_rust_string_lossy(scope))
        }
        v8::PromiseState::Rejected => Err(SandboxError::JsError(
            promise.result(scope).to_rust_string_lossy(scope),
        )),
        v8::PromiseState::Pending => Err(SandboxError::InternalError(
            "module promise still pending after drain".into(),
        )),
    }
}

fn stub_resolve_callback<'s>(
    _context: v8::Local<'s, v8::Context>,
    _specifier: v8::Local<'s, v8::String>,
    _import_attributes: v8::Local<'s, v8::FixedArray>,
    _referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    None
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("javascript error: {0}")]
    JsError(String),

    #[error("internal error: {0}")]
    InternalError(String),
}
