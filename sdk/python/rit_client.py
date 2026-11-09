"""Tiny transport-injected JSON-RPC request builder; no implicit network or credentials."""
class Client:
    def __init__(self, transport):
        self.transport = transport
        self._id = 0
    def request(self, method, params=None):
        self._id += 1
        req = {"jsonrpc": "2.0", "id": self._id, "method": method}
        if params is not None:
            req["params"] = params
        response = self.transport(req)
        if response.get("id") != self._id or response.get("jsonrpc") != "2.0":
            raise ValueError("invalid JSON-RPC response")
        if "error" in response:
            raise RuntimeError(response["error"].get("message", "RPC error"))
        if "result" not in response:
            raise ValueError("missing JSON-RPC result")
        return response["result"]
    def initialize(self):
        return self.request("initialize", {"protocolVersion":"2025-03-26", "capabilities":{}, "clientInfo":{"name":"rit-python", "version":"0.1.0"}})
