use std::io::Read;

use clap::{Parser, Subcommand};
use figure_8_bin::schemas::{
    BrowserCapability, Capabilities, ExecutionRequest, ExecutionResponse, ExecutionResponses,
    McpCapability, NegotiationResponse,
};

#[derive(Debug, Parser)]
struct Cli {
    #[arg(short, long, env = "F8_URL", default_value = "http://localhost:8080")]
    url: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Create {
        #[arg(long)]
        browser: bool,

        #[arg(long)]
        mcp: Vec<String>,
    },

    Exec {
        #[arg(short, long)]
        session: String,

        code: Option<String>,
    },

    Delete {
        #[arg(short, long)]
        session: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let client = reqwest::Client::new();
    let base = cli.url.trim_end_matches('/');

    if let Err(e) = async {
        match cli.command {
            Command::Create { browser, mcp } => {
                let capabilities = Capabilities {
                    fs: None,
                    fetch: None,
                    browser: browser.then_some(BrowserCapability {}),
                    mcp: mcp
                        .into_iter()
                        .map(|server| McpCapability { server })
                        .collect(),
                };

                let negotiation_response = client
                    .post(format!("{base}/session"))
                    .header("content-type", "application/json")
                    .body(serde_json::to_string(&capabilities)?)
                    .send()
                    .await?
                    .text()
                    .await?;

                match serde_json::from_str::<NegotiationResponse>(&negotiation_response)? {
                    NegotiationResponse::Success { session_id, .. } => {
                        if let Some(id) = session_id {
                            println!("{id}");
                        }
                    }
                    NegotiationResponse::Error { message } => {
                        return Err::<_, Box<dyn std::error::Error>>(message.into());
                    }
                }
            }
            Command::Exec { session, code } => {
                let _negotiation_response = client
                    .get(format!("{base}/session/{session}"))
                    .send()
                    .await?
                    .error_for_status()?;

                let code = match code {
                    Some(c) => c,
                    None => {
                        let mut buf = String::new();
                        std::io::stdin().read_to_string(&mut buf)?;
                        buf
                    }
                };

                let execution_responses = client
                    .patch(format!("{base}/session/{session}"))
                    .header("content-type", "application/json")
                    .body(serde_json::to_string(&ExecutionRequest { code })?)
                    .send()
                    .await?
                    .text()
                    .await?;

                for response in
                    serde_json::from_str::<ExecutionResponses>(&execution_responses)?.responses
                {
                    match response {
                        ExecutionResponse::Console { message } => {
                            if message.level >= 2 {
                                eprintln!("{}", message.message);
                            } else {
                                println!("{}", message.message);
                            }
                        }
                        ExecutionResponse::Error { message } => return Err(message.into()),
                    }
                }
            }
            Command::Delete { session } => {
                client
                    .delete(format!("{base}/session/{session}"))
                    .send()
                    .await?
                    .error_for_status()?;
            }
        }
        Ok(())
    }
    .await
    {
        eprintln!("{}", e);
    }
}
