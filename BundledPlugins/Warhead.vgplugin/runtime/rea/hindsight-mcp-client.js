/** Warhead bridge to HindSight's authenticated stdio MCP server. */
import { spawn } from 'node:child_process';
import { connect } from 'node:net';
import { createInterface } from 'node:readline';
import { lstatSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const obj = (v) => typeof v === 'object' && v !== null ? v : {};
async function requestLocalGateway() {
    if (process.platform !== 'darwin')
        return null;
    const socketPath = join(homedir(), 'Library', 'Caches', 'HindSight', 'local-gateway.sock');
    try {
        const directory = lstatSync(dirname(socketPath));
        const socket = lstatSync(socketPath);
        const uid = typeof process.getuid === 'function' ? process.getuid() : undefined;
        if (!directory.isDirectory() || !socket.isSocket() || (uid !== undefined && (directory.uid !== uid || socket.uid !== uid)) ||
            (directory.mode & 0o077) !== 0 || (socket.mode & 0o077) !== 0)
            throw new Error('HindSight local control socket permissions are unsafe');
    }
    catch (error) {
        if (['ENOENT', 'ENOTDIR'].includes(error?.code ?? ''))
            return null;
        throw error;
    }
    const response = await new Promise((resolveReply, reject) => {
        const socket = connect(socketPath);
        let data = '';
        const timer = setTimeout(() => socket.destroy(new Error('HindSight local gateway request timed out')), 5000);
        socket.once('connect', () => socket.write('{"action":"ensure"}\n'));
        socket.on('data', chunk => {
            data += chunk.toString('utf8');
            if (data.length > 8192) {
                clearTimeout(timer);
                socket.destroy(new Error('HindSight local gateway response is too large'));
                return;
            }
            const newline = data.indexOf('\n');
            if (newline >= 0) {
                clearTimeout(timer);
                socket.end();
                try {
                    resolveReply(obj(JSON.parse(data.slice(0, newline))));
                }
                catch {
                    reject(new Error('Invalid HindSight local gateway response'));
                }
            }
        });
        socket.once('error', error => { clearTimeout(timer); reject(error); });
    });
    if (typeof response['error'] === 'string')
        throw new Error(response['error']);
    if (typeof response['url'] !== 'string' || typeof response['token'] !== 'string' || !/^[a-f0-9]{64}$/.test(response['token']) ||
        typeof response['ca'] !== 'string' || !response['ca'].startsWith('-----BEGIN CERTIFICATE-----') ||
        typeof response['certificateSHA256'] !== 'string' || !/^[a-f0-9]{64}$/.test(response['certificateSHA256']))
        throw new Error('HindSight returned an invalid local gateway session');
    return { url: response['url'], token: response['token'], ca: response['ca'], certificateSHA256: response['certificateSHA256'] };
}
function bundledServerPath() {
    const here = dirname(fileURLToPath(import.meta.url));
    const candidates = [resolve(here, '../mcp/hindsight-mcp.mjs'), resolve(here, '../../mcp/hindsight-mcp.mjs'), resolve(here, '../../../HindSight/mcp/hindsight-mcp.mjs')];
    return candidates.find(path => { try { return lstatSync(path).isFile(); }
    catch { return false; } });
}
export class HindsightMcpClient {
    pending = new Map();
    id = 1;
    closed = false;
    child;
    constructor(child) {
        this.child = child;
        createInterface({ input: child.stdout }).on('line', line => {
            let msg;
            try {
                msg = JSON.parse(line);
            }
            catch {
                return;
            }
            if (typeof msg['id'] !== 'number')
                return;
            const p = this.pending.get(msg['id']);
            if (!p)
                return;
            this.pending.delete(msg['id']);
            clearTimeout(p.timer);
            const error = obj(msg['error']);
            if (Object.keys(error).length)
                p.reject(new Error(String(error['message'] ?? 'MCP request failed')));
            else
                p.resolve(msg['result']);
        });
        child.stderr.on('data', data => process.stderr.write(data));
        child.on('error', e => this.fail(e));
        child.on('exit', (code, signal) => { this.closed = true; this.fail(new Error(`HindSight MCP exited (${signal ?? code})`)); });
    }
    static async start() {
        const manual = process.env['WARHEAD_HINDSIGHT_MCP_SERVER']?.trim();
        const script = manual || bundledServerPath();
        if (!script) {
            if (manual)
                throw new Error('HindSight MCP server script was not found');
            return null;
        }
        let url = process.env['HINDSIGHT_URL']?.trim();
        let token = process.env['HINDSIGHT_TOKEN']?.trim();
        let local = null;
        if (!url || !token) {
            if (manual)
                throw new Error('Set HINDSIGHT_URL and HINDSIGHT_TOKEN');
            local = await requestLocalGateway();
            if (!local)
                return null;
            url = local.url;
            token = local.token;
        }
        const env = { ...process.env, HINDSIGHT_URL: url, HINDSIGHT_TOKEN: token };
        if (local) {
            env['HINDSIGHT_CA_PEM'] = local.ca;
            env['HINDSIGHT_CERT_SHA256'] = local.certificateSHA256;
        }
        const ca = process.env['HINDSIGHT_CA_FILE']?.trim();
        if (ca)
            env['HINDSIGHT_CA_FILE'] = ca;
        const child = spawn(process.execPath, [script], { cwd: process.cwd(), env, stdio: ['pipe', 'pipe', 'pipe'] });
        const client = new HindsightMcpClient(child);
        try {
            const init = obj(await client.request('initialize', { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'warhead', version: '3.0.0' } }));
            if (!init['protocolVersion'])
                throw new Error('Invalid HindSight MCP initialize response');
            client.notify('notifications/initialized');
            return client;
        }
        catch (error) {
            await client.close();
            throw error;
        }
    }
    async tools() {
        const listed = obj(await this.request('tools/list', {}));
        const tools = (Array.isArray(listed['tools']) ? listed['tools'] : []);
        return tools.filter(t => typeof t?.name === 'string').map(t => ({
            def: { name: `hindsight_${t.name.replace(/^hindsight_/, '')}`, description: t.description ?? t.name, parameters: t.inputSchema ?? { type: 'object', properties: {} } },
            mutates: /submit|cancel|write|delete|import|patch|execute/i.test(t.name),
            run: async (args, ctx) => {
                const result = obj(await this.request('tools/call', { name: t.name, arguments: args }, 125_000, ctx.signal));
                const blocks = Array.isArray(result['content']) ? result['content'] : [];
                const output = blocks.map(b => obj(b)).filter(b => b['type'] === 'text').map(b => String(b['text'] ?? '')).join('\n') || JSON.stringify(result['structuredContent'] ?? result);
                return result['isError'] === true ? { ok: false, output, error: output } : { ok: true, output };
            },
        }));
    }
    request(method, params, timeout = 30000, signal) {
        if (this.closed || !this.child.stdin.writable)
            return Promise.reject(new Error('HindSight MCP is not running'));
        if (signal?.aborted)
            return Promise.reject(new Error('MCP request cancelled'));
        const id = this.id++;
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => finish(new Error(`MCP ${method} timed out`)), timeout);
            const finish = (error) => { const p = this.pending.get(id); if (!p)
                return; this.pending.delete(id); clearTimeout(p.timer); signal?.removeEventListener('abort', abort); if (error)
                reject(error); };
            const abort = () => finish(new Error('MCP request cancelled'));
            this.pending.set(id, { resolve: v => { signal?.removeEventListener('abort', abort); resolve(v); }, reject, timer });
            signal?.addEventListener('abort', abort, { once: true });
            this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`, e => { if (e)
                finish(e); });
        });
    }
    notify(method) { if (!this.closed && this.child.stdin.writable)
        this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method })}\n`); }
    fail(error) { for (const [id, p] of this.pending) {
        this.pending.delete(id);
        clearTimeout(p.timer);
        p.reject(error);
    } }
    async close() {
        if (this.closed)
            return;
        this.notify('exit');
        this.closed = true;
        if (this.child.exitCode === null && this.child.signalCode === null) {
            this.child.kill('SIGTERM');
            await new Promise(resolve => { const timer = setTimeout(() => { this.child.kill('SIGKILL'); resolve(); }, 2000); this.child.once('exit', () => { clearTimeout(timer); resolve(); }); });
        }
        this.fail(new Error('HindSight MCP closed'));
    }
}
