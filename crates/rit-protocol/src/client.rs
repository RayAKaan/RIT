//! Minimal synchronous, transport-injected Rust JSON-RPC client.
//! It supplies no authentication and is not a full MCP SDK.
use serde_json::{Value, json};
use std::{error::Error, fmt};
/// Client or transport failure; never includes request/response contents.
#[derive(Debug)]
pub enum ClientError<E> {
    /// Transport failed.
    Transport(E),
    /// Response violated the minimal JSON-RPC contract.
    InvalidResponse,
    /// Server returned a numeric error code.
    Remote(i64),
    /// The request ID counter cannot advance.
    IdExhausted,
}
impl<E: fmt::Display> fmt::Display for ClientError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "transport error: {e}"),
            Self::InvalidResponse => write!(f, "invalid RPC response"),
            Self::Remote(code) => write!(f, "remote RPC error {code}"),
            Self::IdExhausted => write!(f, "request ID exhausted"),
        }
    }
}
impl<E: Error + 'static> Error for ClientError<E> {}
/// Client using a caller-supplied real or mock transport.
pub struct Client<T> {
    transport: T,
    next_id: u64,
}
impl<T, E> Client<T>
where
    T: FnMut(Value) -> Result<Value, E>,
{
    /// Create a client without connecting or storing credentials.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            next_id: 0,
        }
    }
    /// Send one request. No automatic retries are performed.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, ClientError<E>> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(ClientError::IdExhausted)?;
        let id = self.next_id;
        let response =
            (self.transport)(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .map_err(ClientError::Transport)?;
        if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || response.get("id").and_then(Value::as_u64) != Some(id)
        {
            return Err(ClientError::InvalidResponse);
        }
        if response.get("result").is_some() == response.get("error").is_some() {
            return Err(ClientError::InvalidResponse);
        }
        if let Some(err) = response.get("error") {
            return Err(ClientError::Remote(
                err.get("code")
                    .and_then(Value::as_i64)
                    .ok_or(ClientError::InvalidResponse)?,
            ));
        }
        response
            .get("result")
            .cloned()
            .ok_or(ClientError::InvalidResponse)
    }
}
