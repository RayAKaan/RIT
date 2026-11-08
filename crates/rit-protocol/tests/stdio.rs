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
