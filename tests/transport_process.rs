//! Exercise framing failures against the actual release entry point.

use std::{process::Stdio, time::Duration};

use serenity_concept2_mcp::transport::MAX_FRAME_BYTES;
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};

const FAKE_TOKEN: &str = "synthetic-process-token";
const PRIVATE_INPUT: &str = "PRIVATE_INPUT_MUST_NOT_REACH_STDERR";

async fn rejected_input(input: Vec<u8>, close_stdin: bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_serenity-concept2-mcp"))
        .env("CONCEPT2_ACCESS_TOKEN", FAKE_TOKEN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("the built executable should start");
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // Oversized input may cause the child to close the pipe before this write
    // completes. Either outcome is valid; the process must still terminate.
    let _ = timeout(Duration::from_secs(5), stdin.write_all(&input))
        .await
        .expect("writing test input must not hang");
    let kept_open = if close_stdin {
        drop(stdin);
        None
    } else {
        // Keep the pipe open to prove oversized frames fail without needing EOF.
        Some(stdin)
    };
    let output = timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .expect("invalid framing must cause prompt process exit")
        .expect("the child output should be collected");
    drop(kept_open);
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "no protocol message should be emitted"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("MCP initialization failed"));
    assert!(!stderr.contains(PRIVATE_INPUT));
    assert!(!stderr.contains(FAKE_TOKEN));
}

#[tokio::test]
async fn oversized_initialization_exits_without_a_newline_or_eof() {
    let mut input = PRIVATE_INPUT.as_bytes().to_vec();
    input.resize(MAX_FRAME_BYTES + 1, b'x');
    rejected_input(input, false).await;
}

#[tokio::test]
async fn incomplete_initialization_exits_with_sanitized_diagnostics() {
    let input = format!("{{\"jsonrpc\":\"2.0\",\"private\":\"{PRIVATE_INPUT}\"").into_bytes();
    rejected_input(input, true).await;
}
