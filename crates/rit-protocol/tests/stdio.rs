use std::io::Write;
use std::process::{Command, Stdio};
#[test]
fn stdio_discovery_and_denied_tool_call() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rit-protocol"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(concat!(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#, "\n", r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"publish"}}"#, "\n").as_bytes()).unwrap();
    drop(input);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let lines: Vec<serde_json::Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(lines[1]["error"]["code"], -32001);
}

#[test]
fn rust_client_interoperates_with_real_stdio_server() {
    use std::io::{BufRead, BufReader};
    let mut child = Command::new(env!("CARGO_BIN_EXE_rit-protocol"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut client = rit_protocol::client::Client::new(
        |request: serde_json::Value| -> std::io::Result<serde_json::Value> {
            serde_json::to_writer(&mut input, &request)?;
            input.write_all(b"\n")?;
            input.flush()?;
            let mut line = String::new();
            output.read_line(&mut line)?;
            serde_json::from_str(&line).map_err(std::io::Error::other)
        },
    );
    let metadata = serde_json::json!({"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}});
    let discovery = client.request("server/discover", metadata.clone()).unwrap();
    assert_eq!(discovery["supportedVersions"][0], "2026-07-28");
    assert_eq!(
        client.request("tools/list", metadata.clone()).unwrap()["tools"],
        serde_json::json!([])
    );
    assert!(matches!(
        client.request("tools/call", metadata),
        Err(rit_protocol::client::ClientError::Remote(-32001))
    ));
    drop(input);
    assert!(child.wait().unwrap().success());
}
