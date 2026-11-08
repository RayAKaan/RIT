//! Minimal local MCP stdio discovery. No privileged operation is enabled.
#![forbid(unsafe_code)]
use serde_json::{Value, json};
/// Supported MCP protocol version.
pub const MCP_VERSION: &str = "2025-03-26";
/// Every valid route is checked by this gateway; the production gateway permits only discovery.
fn authorize(method: &str) -> bool {
    matches!(
        method,
        "initialize" | "notifications/initialized" | "ping" | "tools/list"
    )
}
/// Handle one JSON-RPC request. `None` means no reply, as for notifications.
/// No request parameters are logged or stored. Batch requests are not supported.
pub fn dispatch(input: &str) -> Option<Value> {
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
            .is_some_and(|v| !(v.is_string() || v.is_number() || v.is_null()))
    {
        return Some(error(Value::Null, -32600, "Invalid Request"));
    }
    let method = o["method"].as_str().expect("validated");
    // Middleware runs on every route, including notifications and discovery.
    if !authorize(method) {
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
        "ping" => json!({}),
        "tools/list" => json!({"tools":[]}),
        _ => return Some(error(id, -32601, "Method not found")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
/// Rust SDK request builder; clients choose and authenticate their own transport.
pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

#[cfg(test)]
mod tests {
    use super::*;
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
