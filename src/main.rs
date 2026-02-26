use figure_8::{FromV8, Interface, IntoV8, Sandbox, SandboxResult, ToolDef, ToolError};
use figure_8_macros::tool;

#[tool(description = "Add two numbers")]
fn add(a: f64, b: f64) -> Result<f64, ToolError> {
    Ok(a + b)
}

#[tool(description = "Greet someone by name")]
fn greet(name: String) -> Result<String, ToolError> {
    Ok(format!("Hello, {}!", name))
}

#[tool(description = "Repeat a string n times (default 2)")]
fn repeat(text: String, n: Option<f64>) -> Result<String, ToolError> {
    let count = n.unwrap_or(2.0) as usize;
    Ok(text.repeat(count))
}

#[tool(description = "Wait then greet")]
async fn delayed_greet(name: String, ms: f64) -> Result<String, ToolError> {
    tokio::time::sleep(tokio::time::Duration::from_millis(ms as u64)).await;
    Ok(format!("Hello after {}ms, {}!", ms, name))
}

fn manual_multiply() -> ToolDef {
    ToolDef::new_sync(
        "multiply",
        "Multiply two numbers",
        "declare function multiply(a: number, b: number): number;",
        Box::new(|scope, args, mut rv| {
            let a = f64::from_v8(scope, args.get(0)).unwrap();
            let b = f64::from_v8(scope, args.get(1)).unwrap();
            rv.set((a * b).into_v8(scope));
        }),
    )
}

#[tokio::main]
async fn main() {
    let interface = Interface::builder("demo")
        .tool(add_tool_def())
        .tool(greet_tool_def())
        .tool(repeat_tool_def())
        .tool(delayed_greet_tool_def())
        .tool(manual_multiply())
        .build();

    println!("{}", interface.generate_dts());

    let sandbox = Sandbox::new(interface, tokio::runtime::Handle::current());

    for code in [
        "add(2, 3)",
        "greet('world')",
        "multiply(6, 7)",
        "repeat('ha', 3)",
        "repeat('yo')",
        "await delayed_greet('async', 50)",
        "nonexistent()",
    ] {
        match sandbox.execute(code).await {
            SandboxResult::Ok(val) => println!("  {code} => {val}"),
            SandboxResult::JsError(err) => println!("  {code} => error: {err}"),
            SandboxResult::InternalError(err) => println!("  {code} => internal: {err}"),
        }
    }

    sandbox.shutdown();
}
