use std::collections::HashMap;

use flume::Sender;

use crate::{
    FnDefError, TsType,
    sandbox::fn_def::{FnDef, FnHandler, PendingPromise},
};

pub(crate) struct InjectedFnData {
    fn_def: FnDef,
    pending_tx: *const Sender<PendingPromise>,
}

#[derive(Default)]
struct Node {
    fn_defs: HashMap<String, FnDef>,
    children: HashMap<String, Node>,
    dts_extras: Vec<String>,
}

#[derive(Default)]
pub struct JsApi {
    root: Node,
    polyfills: Vec<String>,
}

pub trait Interface {
    fn extend_api(&self, js_api: &mut JsApi) -> Result<(), FnDefError>;
}

impl JsApi {
    pub fn extend_fn_defs(
        &mut self,
        other: impl IntoIterator<Item = FnDef>,
    ) -> Result<(), FnDefError> {
        for tool in other {
            self.push_fn_def(tool)?;
        }

        Ok(())
    }

    pub fn push_polyfill(&mut self, code: impl Into<String>) {
        self.polyfills.push(code.into());
    }

    pub fn push_dts(&mut self, namespace: &str, dts: impl Into<String>) {
        let dts = dts.into();

        if namespace.is_empty() {
            self.root.dts_extras.push(dts);
        } else {
            let node = namespace.split('.').fold(&mut self.root, |node, part| {
                node.children.entry(part.to_string()).or_default()
            });
            node.dts_extras.push(unwrap_dts_block(&dts));
        }
    }

    pub fn push_fn_def(&mut self, tool: FnDef) -> Result<(), FnDefError> {
        let parts = tool.name.split('.').collect::<Vec<_>>();
        let func_name = *parts.last().unwrap();

        let node = parts[..parts.len() - 1]
            .iter()
            .fold(&mut self.root, |node, part| {
                node.children.entry(part.to_string()).or_default()
            });

        if node.fn_defs.contains_key(func_name) {
            return Err(FnDefError::NameCollision(
                tool.name.clone(),
                func_name.to_string(),
            ));
        }
        node.fn_defs.insert(func_name.to_string(), tool);

        Ok(())
    }

    pub fn generate_dts(&self) -> String {
        fn render(node: &Node, indent: usize, is_root: bool) -> String {
            let pad = "  ".repeat(indent);
            let mut out = String::new();

            for (leaf_name, tool) in &node.fn_defs {
                if leaf_name.starts_with("__") {
                    continue;
                }

                let params = tool
                    .params
                    .iter()
                    .map(|(name, ty)| match ty {
                        TsType::Optional(inner) => format!("{name}?: {inner}"),
                        _ => format!("{name}: {ty}"),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                if !tool.description.is_empty() {
                    out.push_str(&format!("{pad}/** {} */\n", tool.description));
                }

                let decl = if is_root { "declare " } else { "" };
                out.push_str(&format!(
                    "{pad}{decl}function {leaf_name}({params}): {};\n",
                    tool.ret
                ));
            }

            for extra in &node.dts_extras {
                for line in extra.lines() {
                    if line.is_empty() {
                        out.push('\n');
                    } else {
                        out.push_str(&pad);
                        out.push_str(line);
                        out.push('\n');
                    }
                }
            }

            for (name, child) in &node.children {
                let decl = if is_root { "declare " } else { "" };
                out.push_str(&format!("{pad}{decl}namespace {name} {{\n"));
                out.push_str(&render(child, indent + 1, false));
                out.push_str(&format!("{pad}}}\n"));
            }

            out
        }

        render(&self.root, 0, true)
    }

    pub(crate) fn inject(
        self,
        scope: &mut v8::PinScope<'_, '_>,
        global: v8::Local<v8::Object>,
        pending_tx: *const Sender<PendingPromise>,
    ) -> Vec<*mut InjectedFnData> {
        let JsApi {
            root, polyfills, ..
        } = self;
        let mut ptrs = Vec::new();
        let mut stack = vec![(root, global)];

        while let Some((node, target)) = stack.pop() {
            for (func_name, fn_def) in node.fn_defs {
                let key = v8::String::new(scope, &func_name).unwrap();

                let data = Box::into_raw(Box::new(InjectedFnData { fn_def, pending_tx }));
                let external = v8::External::new(scope, data as *mut std::ffi::c_void);
                let func = v8::Function::builder(fn_callback)
                    .data(external.into())
                    .build(scope)
                    .unwrap();

                target.set(scope, key.into(), func.into());
                ptrs.push(data);
            }

            for (name, child) in node.children {
                let obj = v8::Object::new(scope);
                let key = v8::String::new(scope, &name).unwrap();
                target.set(scope, key.into(), obj.into());
                stack.push((child, obj));
            }
        }

        for code in polyfills {
            let source = v8::String::new(scope, &code).unwrap();
            let script = v8::Script::compile(scope, source, None)
                .expect("preamble script failed to compile");
            script.run(scope);
        }

        ptrs
    }
}

pub(crate) unsafe fn free_injected_data(ptrs: Vec<*mut InjectedFnData>) {
    for ptr in ptrs {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

/// Strip a `declare namespace X { ... }` wrapper, returning the de-indented body.
fn unwrap_dts_block(dts: &str) -> String {
    let lines: Vec<&str> = dts.lines().collect();

    let open = lines
        .iter()
        .position(|l| l.trim_end().ends_with('{'))
        .unwrap_or(0);
    let close = lines
        .iter()
        .rposition(|l| l.trim() == "}")
        .unwrap_or(lines.len());

    lines[open + 1..close]
        .iter()
        .map(|line| {
            if line.chars().all(|c| c.is_whitespace()) {
                ""
            } else {
                line.strip_prefix("  ").unwrap_or(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fn_callback(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    rv: v8::ReturnValue,
) {
    let external: v8::Local<v8::External> = args.data().try_into().unwrap();
    let data = unsafe { &*(external.value() as *const InjectedFnData) };

    match &data.fn_def.handler {
        FnHandler::Sync(cb) => cb(scope, args, rv),
        FnHandler::Async(cb) => {
            cb(scope, args, rv, unsafe { &*data.pending_tx });
        }
    }
}
