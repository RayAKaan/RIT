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

#[test]
fn scoped_mcp_reads_real_repository_and_denies_mutations_and_paths() {
    use std::{
        fs,
        io::{BufRead, BufReader},
    };
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    fs::create_dir(&repo).unwrap();
    let git = |a: &[&str]| {
        let s = Command::new("git")
            .args(a)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .status()
            .unwrap();
        assert!(s.success());
    };
    git(&["init", "-q"]);
    fs::write(repo.join("a.py"), "import os\n").unwrap();
    git(&["add", "--", "a.py"]);
    git(&["commit", "-q", "-m", "initial"]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_rit-protocol"))
        .arg("--repo")
        .arg(&repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut call = |value: serde_json::Value| {
        serde_json::to_writer(&mut stdin, &value).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
        let mut s = String::new();
        stdout.read_line(&mut s).unwrap();
        serde_json::from_str::<serde_json::Value>(&s).unwrap()
    };
    let list = call(serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 6);
    let result = call(
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"repository.inspect","arguments":{}}}),
    );
    assert_eq!(
        result["result"]["structuredContent"]["inspection"]["object_format"],
        "sha1"
    );
    let graph = call(
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"dependencies.inspect","arguments":{}}}),
    );
    assert_eq!(
        graph["result"]["structuredContent"]["graph"]["files"][0],
        "a.py"
    );
    let escape = call(
        serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"repository.inspect","arguments":{"path":".."}}}),
    );
    assert_eq!(escape["error"]["code"], -32602);
    let denied = call(
        serde_json::json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"integration.publish","arguments":{}}}),
    );
    assert_eq!(denied["error"]["code"], -32001);
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn rust_client_reads_scoped_repository_through_real_stdio() {
    use std::io::{BufRead, BufReader};
    let d = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(d.path())
            .status()
            .unwrap()
            .success()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_rit-protocol"))
        .arg("--repo")
        .arg(d.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut client = rit_protocol::client::Client::new(
        |req: serde_json::Value| -> std::io::Result<serde_json::Value> {
            serde_json::to_writer(&mut input, &req)?;
            input.write_all(b"\n")?;
            input.flush()?;
            let mut line = String::new();
            output.read_line(&mut line)?;
            serde_json::from_str(&line).map_err(std::io::Error::other)
        },
    );
    let result = client
        .request(
            "tools/call",
            serde_json::json!({"name":"repository.inspect","arguments":{}}),
        )
        .unwrap();
    assert_eq!(
        result["structuredContent"]["inspection"]["object_format"],
        "sha1"
    );
    assert!(
        client
            .request(
                "tools/call",
                serde_json::json!({"name":"integration.publish","arguments":{}})
            )
            .is_err()
    );
    drop(input);
    assert!(child.wait().unwrap().success());
}
