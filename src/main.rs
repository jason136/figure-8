use figure_8::{Interface, Sandbox, SandboxResult, browser, tool_sync};

fn log_tool() -> figure_8::ToolDef {
    tool_sync!("log", "Log a message", |msg: String| -> () {
        println!("[agent] {msg}");
        Ok(())
    })
}

#[tokio::main]
async fn main() {
    let browser = browser::Browser::new(browser::Browser::default_config());

    let interface = browser::register_tools(
        Interface::builder("browser_agent").tool(log_tool()),
        &browser,
    )
    .build();

    println!(
        "--- TypeScript declarations ---\n{}",
        interface.generate_dts()
    );

    let sandbox = Sandbox::new(interface, tokio::runtime::Handle::current());

    let code = r#"
        await page.launch();
        await page.goto("https://example.com");
        const title = await page.getTitle();
        log("Page title: " + title);
        const heading = await page.getText("h1");
        log("Heading: " + heading);
        heading
    "#;

    println!("--- Running browser agent ---");
    match sandbox.execute(code).await {
        SandboxResult::Ok(val) => println!("Result: {val}"),
        SandboxResult::JsError(err) => println!("JS Error: {err}"),
        SandboxResult::InternalError(err) => println!("Internal Error: {err}"),
    }

    sandbox.shutdown();
}
