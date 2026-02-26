use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Once;

use tokio::sync::{mpsc, oneshot};

use interface::Interface;
use tool::Pending;

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

pub enum SandboxCmd {
    Execute {
        code: String,
        reply: oneshot::Sender<SandboxResult>,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum SandboxResult {
    Ok(String),
    JsError(String),
    InternalError(String),
}

pub struct Sandbox {
    cmd_tx: mpsc::UnboundedSender<SandboxCmd>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Sandbox {
    pub fn new(interface: Interface, tokio_handle: tokio::runtime::Handle) -> Self {
        ensure_v8();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let handle = std::thread::spawn(move || {
            let _guard = tokio_handle.enter();
            run_isolate(interface, cmd_rx);
        });

        Sandbox {
            cmd_tx,
            handle: Some(handle),
        }
    }

    pub async fn execute(&self, code: &str) -> SandboxResult {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SandboxCmd::Execute {
                code: code.to_string(),
                reply: tx,
            })
            .is_err()
        {
            return SandboxResult::InternalError("sandbox thread has exited".into());
        }
        rx.await.unwrap_or(SandboxResult::InternalError(
            "sandbox dropped reply channel".into(),
        ))
    }

    pub fn shutdown(mut self) {
        let _ = self.cmd_tx.send(SandboxCmd::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(SandboxCmd::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn run_isolate(interface: Interface, mut cmd_rx: mpsc::UnboundedReceiver<SandboxCmd>) {
    let mut isolate = v8::Isolate::new(Default::default());
    let pending: Box<Pending> = Box::new(RefCell::new(VecDeque::new()));
    let pending_ptr: *const Pending = &*pending;

    let (context_global, injected_ptrs) = {
        let scope = std::pin::pin!(v8::HandleScope::new(&mut isolate));
        let scope = &mut scope.init();
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let global = context.global(scope);
        let ptrs = interface.inject(scope, global, pending_ptr);
        (v8::Global::new(scope, context), ptrs)
    };

    loop {
        let cmd = match cmd_rx.blocking_recv() {
            Some(cmd) => cmd,
            None => break,
        };

        match cmd {
            SandboxCmd::Shutdown => break,
            SandboxCmd::Execute { code, reply } => {
                let result = execute(&mut isolate, &context_global, &code, &pending);
                let _ = reply.send(result);
            }
        }
    }

    drop(context_global);
    drop(isolate);
    unsafe { interface::free_injected_data(injected_ptrs) };
}

fn execute(
    isolate: &mut v8::Isolate,
    context: &v8::Global<v8::Context>,
    code: &str,
    pending: &Pending,
) -> SandboxResult {
    // Wrap in a module that exports the result of the last expression.
    let wrapped = format!("const __result = {code}; globalThis.__result = __result;");

    let module_promise = {
        let scope = std::pin::pin!(v8::HandleScope::new(isolate));
        let scope = &mut scope.init();
        let ctx = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, ctx);

        let name = v8::String::new(scope, "<sandbox>").unwrap();
        let source_str = match v8::String::new(scope, &wrapped) {
            Some(s) => s,
            None => return SandboxResult::InternalError("failed to create V8 string".into()),
        };

        let origin = v8::ScriptOrigin::new(
            scope,
            name.into(),
            0,     // line offset
            0,     // column offset
            false, // is_shared_cross_origin
            -1,    // script_id
            None,  // source_map_url
            false, // is_opaque
            false, // is_wasm
            true,  // is_module
            None,  // host_defined_options
        );

        let mut source = v8::script_compiler::Source::new(source_str, Some(&origin));

        let tc = v8::TryCatch::new(scope);
        let tc = std::pin::pin!(tc);
        let tc = &mut tc.init();

        let module = match v8::script_compiler::compile_module(tc, &mut source) {
            Some(m) => m,
            None => {
                let msg = tc
                    .exception()
                    .map(|e| e.to_rust_string_lossy(tc))
                    .unwrap_or_else(|| "compilation error".into());
                return SandboxResult::JsError(msg);
            }
        };

        if module
            .instantiate_module(tc, stub_resolve_callback)
            .is_none()
        {
            let msg = tc
                .exception()
                .map(|e| e.to_rust_string_lossy(tc))
                .unwrap_or_else(|| "module instantiation error".into());
            return SandboxResult::JsError(msg);
        }

        let result = match module.evaluate(tc) {
            Some(val) => val,
            None => {
                let msg = tc
                    .exception()
                    .map(|e| e.to_rust_string_lossy(tc))
                    .unwrap_or_else(|| "evaluation error".into());
                return SandboxResult::JsError(msg);
            }
        };

        let promise: v8::Local<v8::Promise> = result.try_into().unwrap();
        v8::Global::new(tc, promise)
    };

    // Drain pending async ops, resolving tool promises.
    // Each resolution may trigger microtasks that call more tools.
    loop {
        let p = pending.borrow_mut().pop_front();
        let Some(p) = p else { break };

        let result = match p.rx.blocking_recv() {
            Ok(r) => r,
            Err(_) => Err("tool future dropped".into()),
        };

        let scope = std::pin::pin!(v8::HandleScope::new(isolate));
        let scope = &mut scope.init();
        let ctx = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, ctx);

        let resolver = v8::Local::new(scope, &p.resolver);
        match result {
            Ok(deferred) => {
                let val = deferred.to_v8(scope);
                resolver.resolve(scope, val);
            }
            Err(msg) => {
                let err = v8::String::new(scope, &msg).unwrap();
                resolver.reject(scope, v8::Exception::error(scope, err));
            }
        }
        scope.perform_microtask_checkpoint();
    }

    // Read the module's final result from its top-level promise.
    let scope = std::pin::pin!(v8::HandleScope::new(isolate));
    let scope = &mut scope.init();
    let ctx = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, ctx);

    let promise = v8::Local::new(scope, &module_promise);
    match promise.state() {
        v8::PromiseState::Fulfilled => {
            let key = v8::String::new(scope, "__result").unwrap();
            let global = ctx.global(scope);
            let val = global.get(scope, key.into()).unwrap();
            SandboxResult::Ok(val.to_rust_string_lossy(scope))
        }
        v8::PromiseState::Rejected => {
            SandboxResult::JsError(promise.result(scope).to_rust_string_lossy(scope))
        }
        v8::PromiseState::Pending => {
            SandboxResult::InternalError("module promise still pending after drain".into())
        }
    }
}

/// Reject all import requests -- tools are on globalThis, not modules.
fn stub_resolve_callback<'s>(
    _context: v8::Local<'s, v8::Context>,
    _specifier: v8::Local<'s, v8::String>,
    _import_attributes: v8::Local<'s, v8::FixedArray>,
    _referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    None
}
