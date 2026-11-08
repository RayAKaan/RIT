/** Transport-injected JSON-RPC foundation; no implicit network or credentials. */
export type Request = {jsonrpc: '2.0'; id: number; method: string; params?: unknown};
export type Response = {jsonrpc: '2.0'; id: number; result?: unknown; error?: {code: number; message: string}};
export type Transport = (request: Request) => Promise<Response>;
export class Client {
  private id = 0;
  constructor(private readonly transport: Transport) {}
  async request(method: string, params?: unknown): Promise<unknown> {
    const id = ++this.id;
    const response = await this.transport({jsonrpc: '2.0', id, method, params});
    if (response.jsonrpc !== '2.0' || response.id !== id) throw new Error('Invalid JSON-RPC response');
    if (response.error) throw new Error(response.error.message);
    if (!('result' in response)) throw new Error('Missing JSON-RPC result');
    return response.result;
  }
  initialize(): Promise<unknown> {
    return this.request('initialize', {protocolVersion: '2025-03-26', capabilities: {}, clientInfo: {name: 'rit-typescript', version: '0.1.0'}});
  }
}
