//! Minimal local MCP stdio discovery. No privileged operation is enabled.
#![forbid(unsafe_code)]
/// Minimal transport-injected Rust client.
pub mod client;
use serde_json::{Value, json};
/// Supported MCP protocol version.
pub const MCP_VERSION: &str = "2025-03-26";
/// Current modern discovery version; supported only by the discovery prototype, not a complete MCP server.
pub const MODERN_VERSION: &str = "2026-07-28";
/// Every valid route is checked by this gateway; the production gateway permits only discovery.
fn authorize(method: &str) -> bool {
    matches!(
        method,
        "initialize" | "notifications/initialized" | "server/discover" | "ping" | "tools/list"
    )
}
/// Handle one JSON-RPC request. `None` means no reply, as for notifications.
/// No request parameters are logged or stored. Batch requests are not supported.
pub fn dispatch(input: &str) -> Option<Value> {
    dispatch_scoped(input, None)
}
/// Dispatch with an optional explicitly selected local read-only repository scope.
pub fn dispatch_scoped(input: &str, scope: Option<&ReadOnlyScope>) -> Option<Value> {
    let request: Value = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return Some(error(Value::Null, -32700, "Parse error")),
    };
    let Some(o) = request.as_object() else {
        return Some(error(Value::Null, -32600, "Invalid Request"));
    };
    let id = o.get("id").cloned();
    if o.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || o.get("method").and_then(Value::as_str).is_none()
        || id
            .as_ref()
            .is_some_and(|v| !(v.is_string() || v.as_i64().is_some() || v.as_u64().is_some()))
    {
        return Some(error(Value::Null, -32600, "Invalid Request"));
    }
    let method = o["method"].as_str().expect("validated");
    // Detect modern requests via per-request metadata. No request data is logged.
    let meta = o.get("params").and_then(|p| p.get("_meta"));
    let modern = method == "server/discover"
        || meta
            .and_then(|m| m.get("io.modelcontextprotocol/protocolVersion"))
            .is_some();
    if modern {
        let Some(version) = meta
            .and_then(|m| m.get("io.modelcontextprotocol/protocolVersion"))
            .and_then(Value::as_str)
        else {
            return id.map(|id| error(id, -32602, "Missing protocol version"));
        };
        if version != MODERN_VERSION {
            return id.map(|id| json!({"jsonrpc":"2.0","id":id,"error":{"code":-32022,"message":"Unsupported protocol version","data":{"supported":[MODERN_VERSION],"requested":version}}}));
        }
        if !meta
            .and_then(|m| m.get("io.modelcontextprotocol/clientCapabilities"))
            .is_some_and(Value::is_object)
        {
            return id.map(|id| error(id, -32602, "Missing client capabilities"));
        }
    }
    // Middleware runs on every route, including notifications and discovery.
    if !authorize(method) && !(method == "tools/call" && scope.is_some()) {
        return id.map(|id| error(id, -32001, "Operation unavailable or unauthorized"));
    }
    if method == "notifications/initialized" {
        return None;
    }
    let id = id?;
    let result = match method {
        "initialize" => {
            if o.get("params")
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Value::as_str)
                .is_none()
            {
                return Some(error(id, -32602, "Invalid params"));
            }
            json!({"protocolVersion":MCP_VERSION,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"rit","version":env!("CARGO_PKG_VERSION")}})
        }
        "server/discover" => {
            json!({"resultType":"complete", "supportedVersions":[MODERN_VERSION], "capabilities":{"tools":{}}, "_meta":{"io.modelcontextprotocol/serverInfo":{"name":"rit","version":env!("CARGO_PKG_VERSION")}}, "ttlMs":0, "cacheScope":"public"})
        }
        "ping" if modern => json!({"resultType":"complete"}),
        "tools/list" if modern => json!({"resultType":"complete","tools":tool_list(scope)}),
        "ping" => json!({}),
        "tools/list" => json!({"tools":tool_list(scope)}),
        "tools/call" => {
            let params = o.get("params");
            let Some(name) = params.and_then(|p| p.get("name")).and_then(Value::as_str) else {
                return Some(error(id, -32602, "Invalid params"));
            };
            let args = params.and_then(|p| p.get("arguments"));
            if args.is_some_and(|v| !v.as_object().is_some_and(|o| o.is_empty())) {
                return Some(error(id, -32602, "Arguments must be empty"));
            }
            let Some(scope) = scope else {
                return Some(error(id, -32001, "Operation unavailable or unauthorized"));
            };
            let result = match scope.call(name) {
                Ok(v) => v,
                Err(_) => return Some(error(id, -32001, "Operation unavailable or unauthorized")),
            };
            let mut value = json!({"content":[{"type":"text","text":result.to_string()}],"structuredContent":result,"isError":false});
            if modern {
                value["resultType"] = json!("complete");
            }
            value
        }
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
/// Explicit, single-repository stdio scope. This is an OS-user boundary, not RIT authentication.
pub struct ReadOnlyScope {
    repo: std::path::PathBuf,
    common_git_dir: String,
    service: rit_application::GitReadService,
    git: rit_git::Git,
}
impl ReadOnlyScope {
    /// Select a repository once at startup. Tool arguments cannot select another path.
    pub fn open(repo: &std::path::Path) -> Result<Self, String> {
        let repo = repo
            .canonicalize()
            .map_err(|_| "cannot access repository path".to_string())?;
        let git = rit_git::Git::discover(None).map_err(|_| "Git unavailable".to_string())?;
        let service = rit_application::GitReadService::new(git.clone());
        let ins = service
            .inspect(&repo)
            .map_err(|_| "repository unavailable".to_string())?;
        Ok(Self {
            repo,
            common_git_dir: ins.common_git_dir,
            service,
            git,
        })
    }
    fn call(&self, name: &str) -> Result<Value, ()> {
        let ins = self.service.inspect(&self.repo).map_err(|_| ())?;
        if ins.common_git_dir != self.common_git_dir {
            return Err(());
        }
        match name {
            "repository.inspect" => Ok(json!({"schema_version":1,"inspection":ins})),
            "repository.status" => Ok(json!({"schema_version":1,"status":ins.status.ok_or(())?})),
            "repository.branches" => Ok(
                json!({"schema_version":1,"references":self.service.references(&self.repo).map_err(|_|())?.into_iter().filter(|r|r.name.starts_with("refs/heads/")||r.name.starts_with("refs/remotes/")).collect::<Vec<_>>() }),
            ),
            "repository.history" => Ok(
                json!({"schema_version":1,"commits":self.service.history(&self.repo,20).map_err(|_|())?}),
            ),
            "repository.worktrees" => Ok(
                json!({"schema_version":1,"worktrees":self.service.worktrees(&self.repo).map_err(|_|())?}),
            ),
            "dependencies.inspect" => Ok(
                json!({"schema_version":1,"graph":self.service.dependencies(&self.repo).map_err(|_|())?}),
            ),
            "workspace.list" => Ok(
                json!({"schema_version":1,"reconciliation":rit_application::WorkspaceService::new(self.git.clone()).reconcile(&self.repo).map_err(|_|())?,"authenticated":false}),
            ),
            "checkpoint.list" => {
                let service = rit_application::GitCheckpointService::new(self.git.clone());
                Ok(
                    json!({"schema_version":1,"content_checkpoints":service.list_content(&self.repo).map_err(|_|())?,"commit_only":service.list(&self.repo).map_err(|_|())?,"authenticated":false}),
                )
            }
            "submission.list" => Ok(
                json!({"schema_version":1,"submissions":rit_application::submissions::SubmissionService::new(self.git.clone()).list(&self.repo).map_err(|_|())?,"authenticated":false}),
            ),
            "candidate.list" => Ok(
                json!({"schema_version":1,"candidates":rit_application::submissions::SubmissionService::new(self.git.clone()).candidates(&self.repo).map_err(|_|())?,"authenticated":false}),
            ),
            _ => Err(()),
        }
    }
}
fn tool_list(scope: Option<&ReadOnlyScope>) -> Vec<Value> {
    if scope.is_none() {
        return Vec::new();
    }
    ["repository.inspect","repository.status","repository.branches","repository.history","repository.worktrees","dependencies.inspect","workspace.list","checkpoint.list","submission.list","candidate.list"].iter().map(|name|json!({"name":name,"description":"Read-only inspection of the explicitly configured repository; no path arguments.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}})).collect()
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
/// Rust SDK request builder; clients choose and authenticate their own transport.
pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

/// Serve bounded newline-delimited requests on standard input/output, with optional read-only scope.
pub fn serve_stdio(scope: Option<ReadOnlyScope>) -> std::io::Result<()> {
    use std::io::{self, BufRead, Read, Write};
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    loop {
        let mut line = Vec::new();
        let n = (&mut input).take(65_537).read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        if n > 65_536 || !line.ends_with(b"\n") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request too large or unterminated",
            ));
        }
        let text = std::str::from_utf8(&line)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?;
        if let Some(reply) = dispatch_scoped(text, scope.as_ref()) {
            serde_json::to_writer(&mut output, &reply)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_discovery_version_and_denial() {
        let call = |method: &str, version: &str| {
            let request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":{"_meta":{"io.modelcontextprotocol/protocolVersion":version,"io.modelcontextprotocol/clientCapabilities":{}}}});
            dispatch(&request.to_string()).unwrap()
        };
        let result = call("server/discover", MODERN_VERSION);
        assert_eq!(
            result["result"]["supportedVersions"],
            json!([MODERN_VERSION])
        );
        assert_eq!(result["result"]["resultType"], "complete");
        assert_eq!(
            call("tools/list", MODERN_VERSION)["result"]["tools"],
            json!([])
        );
        assert_eq!(call("tools/call", MODERN_VERSION)["error"]["code"], -32001);
        assert_eq!(
            call("server/discover", "1900-01-01")["error"]["code"],
            -32022
        );
        assert_eq!(
            dispatch(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#).unwrap()["error"]["code"],
            -32600
        );
    }
    #[test]
    fn discovery_and_default_deny() {
        let init=dispatch(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#).unwrap();
        assert_eq!(init["result"]["protocolVersion"], MCP_VERSION);
        assert_eq!(
            dispatch(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap()["result"]["tools"],
            json!([])
        );
        assert_eq!(
            dispatch(
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"publish"}}"#
            )
            .unwrap()["error"]["code"],
            -32001
        );
        assert!(dispatch(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert_eq!(dispatch("{").unwrap()["error"]["code"], -32700);
    }
}
