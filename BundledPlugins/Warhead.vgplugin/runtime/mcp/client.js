/**
 * Streamable HTTP MCP client (JSON-RPC over POST with optional SSE responses).
 */
import { asObject, responseText } from './protocol.js';
function expandEnvValue(value) {
    const regex = /\$\{([A-Za-z_][A-Za-z0-9_]*)\}/g;
    let out = value;
    const matches = [];
    let m;
    const re = new RegExp(regex);
    while ((m = re.exec(value)))
        matches.push(m[1]);
    for (const v of matches) {
        const envVal = process.env[v];
        if (envVal === undefined || envVal === '')
            return null;
    }
    out = value.replace(regex, (_all, name) => process.env[name] ?? '');
    return out;
}
function buildHeaders(server, sessionId, protocolVersion) {
    const headers = {
        'Content-Type': 'application/json',
        Accept: 'application/json, text/event-stream',
    };
    if (protocolVersion)
        headers['MCP-Protocol-Version'] = protocolVersion;
    if (sessionId)
        headers['Mcp-Session-Id'] = sessionId;
    const configured = server.headers ?? {};
    for (const [k, v] of Object.entries(configured)) {
        if (typeof v !== 'string')
            continue;
        const expanded = expandEnvValue(v);
        if (expanded === null)
            continue;
        headers[k] = expanded;
    }
    // Ensure defaults exist even if caller didn't set them
    if (!headers['Accept'])
        headers['Accept'] = 'application/json, text/event-stream';
    if (!headers['Content-Type'])
        headers['Content-Type'] = 'application/json';
    return headers;
}
function extractSseBlocks(buffer) {
    const parts = buffer.split(/\r?\n\r?\n/);
    if (parts.length === 0)
        return { blocks: [], rest: buffer };
    const rest = parts.pop() ?? '';
    return { blocks: parts, rest };
}
function parseSseBlock(block) {
    const lines = block.split(/\r?\n/);
    const dataLines = [];
    for (const line of lines) {
        if (line.startsWith('data:')) {
            // Remove leading 'data:' and optional space
            const v = line.slice(5);
            dataLines.push(v.startsWith(' ') ? v.slice(1) : v);
        }
        // Ignore comments/event ids
    }
    if (dataLines.length === 0)
        return null;
    const data = dataLines.join('\n');
    if (data.trim() === '' || data.trim() === '[DONE]')
        return null;
    try {
        return JSON.parse(data);
    }
    catch {
        return null;
    }
}
function matchRpcMessage(value, id) {
    if (Array.isArray(value)) {
        for (const item of value) {
            const m = matchRpcMessage(item, id);
            if (m)
                return m;
        }
        return null;
    }
    const obj = asObject(value);
    if (Object.prototype.hasOwnProperty.call(obj, 'id')) {
        const msgId = obj['id'];
        if (msgId === id) {
            if (obj['error'] && typeof obj['error'] === 'object') {
                const err = asObject(obj['error']);
                throw new Error(String(err['message'] ?? 'MCP request failed'));
            }
            if (Object.prototype.hasOwnProperty.call(obj, 'result')) {
                const res = asObject(obj['result']);
                return { result: res };
            }
        }
    }
    return null;
}
async function* readSseMessages(body) {
    const reader = body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    try {
        while (true) {
            const read = await reader.read();
            if (read.done)
                break;
            if (read.value) {
                buffer += decoder.decode(read.value, { stream: true });
            }
            const { blocks, rest } = extractSseBlocks(buffer);
            buffer = rest;
            for (const block of blocks) {
                const msg = parseSseBlock(block);
                if (msg !== null)
                    yield msg;
            }
        }
        buffer += decoder.decode();
        if (buffer.trim()) {
            const msg = parseSseBlock(buffer);
            if (msg !== null)
                yield msg;
        }
    }
    finally {
        try {
            await reader.cancel();
        }
        catch {
            // ignore
        }
    }
}
export class HttpMcpClient {
    sessionId = null;
    protocolVersion = null;
    nextId = 1;
    closed = false;
    constructor(server, fetchImpl) {
        this.server = server;
        this.fetchImpl = fetchImpl;
    }
    server;
    fetchImpl;
    static async connect(server, opts = {}) {
        const fetchImpl = opts.fetchImpl ?? fetch;
        const client = new HttpMcpClient(server, fetchImpl);
        const timeoutMs = opts.timeoutMs ?? 15000;
        const result = await client.rpc('initialize', {
            protocolVersion: '2025-06-18',
            capabilities: {},
            clientInfo: { name: 'warhead', version: '0.1.0' },
        }, { timeoutMs });
        const obj = asObject(result);
        if (!obj['protocolVersion']) {
            throw new Error('MCP server returned invalid initialize response');
        }
        client.protocolVersion = String(obj['protocolVersion']);
        try {
            await client.notify('notifications/initialized');
        }
        catch {
            // Some servers don't require/accept notification
        }
        return client;
    }
    headers() {
        return buildHeaders(this.server, this.sessionId, this.protocolVersion);
    }
    async notify(method, params) {
        if (this.closed)
            return;
        const body = JSON.stringify({ jsonrpc: '2.0', method, ...(params ? { params } : {}) });
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(new Error('MCP notify timed out')), 10000);
        try {
            const res = await this.fetchImpl(this.server.url, {
                method: 'POST',
                headers: this.headers(),
                body,
                signal: controller.signal,
            });
            const sid = res.headers.get('mcp-session-id');
            if (sid && !this.sessionId)
                this.sessionId = sid;
            // notifications may return 202
        }
        finally {
            clearTimeout(timer);
        }
    }
    async rpc(method, params, opts = {}) {
        if (this.closed)
            throw new Error('MCP client closed');
        if (opts.signal?.aborted)
            throw new Error('MCP request cancelled');
        const id = this.nextId++;
        const payload = { jsonrpc: '2.0', id, method, params };
        const controller = new AbortController();
        const timeoutMs = opts.timeoutMs ?? 30000;
        const timer = setTimeout(() => controller.abort(new Error(`MCP ${method} timed out after ${timeoutMs}ms`)), timeoutMs);
        const onAbort = () => controller.abort(opts.signal?.reason ?? new Error('MCP request cancelled'));
        if (opts.signal)
            opts.signal.addEventListener('abort', onAbort, { once: true });
        try {
            const res = await this.fetchImpl(this.server.url, {
                method: 'POST',
                headers: this.headers(),
                body: JSON.stringify(payload),
                signal: controller.signal,
            });
            const sid = res.headers.get('mcp-session-id');
            if (sid)
                this.sessionId = sid;
            if (!res.ok) {
                const text = await res.text().catch(() => '');
                throw new Error(`MCP ${method} failed (${res.status}): ${text.slice(0, 500)}`);
            }
            const ct = res.headers.get('content-type') ?? '';
            if (ct.includes('text/event-stream') && res.body) {
                for await (const msg of readSseMessages(res.body)) {
                    const matched = matchRpcMessage(msg, id);
                    if (matched)
                        return matched['result'];
                }
                throw new Error('MCP stream ended without response');
            }
            const text = await res.text();
            const parsed = JSON.parse(text);
            const matched = matchRpcMessage(parsed, id);
            if (!matched)
                throw new Error('MCP response missing matching result');
            return matched['result'];
        }
        catch (err) {
            if (controller.signal.aborted && !(opts.signal?.aborted)) {
                const reason = controller.signal.reason;
                if (reason instanceof Error)
                    throw reason;
                throw new Error(`MCP ${method} timed out after ${timeoutMs}ms`);
            }
            if (opts.signal?.aborted)
                throw new Error('MCP request cancelled');
            throw err;
        }
        finally {
            clearTimeout(timer);
            if (opts.signal)
                opts.signal.removeEventListener('abort', onAbort);
        }
    }
    async listTools() {
        const all = [];
        let cursor;
        let pages = 0;
        do {
            const params = {};
            if (typeof cursor === 'string' && cursor !== '')
                params['cursor'] = cursor;
            const result = asObject(await this.rpc('tools/list', params));
            const tools = Array.isArray(result['tools']) ? result['tools'] : [];
            for (const t of tools) {
                const tool = asObject(t);
                if (typeof tool['name'] === 'string' && tool['name'].trim() !== '') {
                    all.push({
                        name: tool['name'],
                        description: typeof tool['description'] === 'string' ? tool['description'] : undefined,
                        inputSchema: asObject(tool['inputSchema']),
                        annotations: tool['annotations'] && typeof tool['annotations'] === 'object' ? tool['annotations'] : undefined,
                    });
                }
            }
            cursor = result['nextCursor'];
            pages++;
            if (pages > 100)
                break;
        } while (typeof cursor === 'string' && cursor !== '');
        return all;
    }
    async callTool(name, args, signal, timeoutMs = 120000) {
        const result = asObject(await this.rpc('tools/call', { name, arguments: args }, { signal, timeoutMs }));
        const output = responseText(result);
        if (result['isError'] === true) {
            return { ok: false, output, error: output };
        }
        return { ok: true, output };
    }
    async close() {
        if (this.closed)
            return;
        this.closed = true;
        if (this.sessionId) {
            const controller = new AbortController();
            const timer = setTimeout(() => controller.abort(), 2000);
            try {
                const response = await this.fetchImpl(this.server.url, {
                    method: 'DELETE',
                    headers: this.headers(),
                    signal: controller.signal,
                });
                await response.body?.cancel();
            }
            catch {
                // ignore
            }
            finally {
                clearTimeout(timer);
            }
        }
    }
}
