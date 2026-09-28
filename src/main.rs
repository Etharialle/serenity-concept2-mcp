use std::{process::ExitCode, sync::Arc};

use clap::Parser;
use rmcp::ServiceExt;
use serenity_concept2_mcp::{config::Cli, server::LogbookServer};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    // Do not enable upstream debug logging: requests and MCP bodies may contain secrets.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let api = match cli.api_client() {
        Ok(api) => api,
        Err(_) => {
            eprintln!(
                "Set CONCEPT2_ACCESS_TOKEN to a valid personal Logbook token in the server's environment."
            );
            return ExitCode::FAILURE;
        }
    };
    let service = match LogbookServer::new(Arc::new(api))
        .serve(rmcp::transport::stdio())
        .await
    {
        Ok(service) => service,
        Err(_) => {
            eprintln!("MCP initialization failed. Check the client's stdio configuration.");
            return ExitCode::FAILURE;
        }
    };
    let cancellation = service.cancellation_token();
    tokio::select! {
        result = service.waiting() => {
            if result.is_err() {
                eprintln!("MCP connection closed with an error.");
                return ExitCode::FAILURE;
            }
        }
        _ = tokio::signal::ctrl_c() => { cancellation.cancel(); }
    }
    ExitCode::SUCCESS
}
