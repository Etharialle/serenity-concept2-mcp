use std::{process::ExitCode, sync::Arc, time::Duration};

use clap::Parser;
use rmcp::ServiceExt;
use serenity_concept2_mcp::{config::Cli, server::LogbookServer, transport::BoundedInput};

const SHUTDOWN_BUDGET: Duration = Duration::from_secs(1);

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Do not enable upstream debug logging: requests and MCP bodies may contain secrets.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("Could not start the server runtime.");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(run(cli));
    // Tokio's stdin reader uses blocking I/O that cannot be cancelled. Bound
    // runtime shutdown so Ctrl+C never waits indefinitely for Enter or EOF.
    runtime.shutdown_timeout(SHUTDOWN_BUDGET);
    result
}

async fn run(cli: Cli) -> ExitCode {
    let api = match cli.api_client() {
        Ok(api) => api,
        Err(_) => {
            eprintln!(
                "Set CONCEPT2_ACCESS_TOKEN to a valid personal Logbook token in the server's environment."
            );
            return ExitCode::FAILURE;
        }
    };
    let transport = (BoundedInput::new(tokio::io::stdin()), tokio::io::stdout());
    let start = LogbookServer::new(Arc::new(api)).serve(transport);
    let started = tokio::select! {
        result = start => result,
        signal = tokio::signal::ctrl_c() => return signal_exit(signal),
    };
    let service = match started {
        Ok(service) => service,
        Err(_) => {
            eprintln!("MCP initialization failed. Check the client's stdio configuration.");
            return ExitCode::FAILURE;
        }
    };
    let cancellation = service.cancellation_token();
    let waiting = service.waiting();
    tokio::pin!(waiting);
    tokio::select! {
        result = &mut waiting => {
            if result.is_err() {
                eprintln!("MCP connection closed with an error.");
                return ExitCode::FAILURE;
            }
        }
        signal = tokio::signal::ctrl_c() => {
            cancellation.cancel();
            // Give in-flight reads and SDK tasks a brief opportunity to close.
            let _ = tokio::time::timeout(SHUTDOWN_BUDGET, &mut waiting).await;
            return signal_exit(signal);
        }
    }
    ExitCode::SUCCESS
}

fn signal_exit(result: std::io::Result<()>) -> ExitCode {
    if result.is_err() {
        eprintln!("Could not listen for the shutdown signal.");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
