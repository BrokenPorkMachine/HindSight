/** Warhead-owned connection to an explicitly installed REA MCP server. */
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
function asObject(value) {
    return typeof value === 'object' && value !== null ? value : {};
}
function responseText(result) {
    const blocks = Array.isArray(result['content']) ? result['content'] : [];
    const parts = blocks.flatMap(block => {
        const item = asObject(block);
        if (item['type'] === 'text' && typeof item['text'] === 'string')
            return [item['text']];
        if (item['type'] === 'image')
            return ['[image result omitted]'];
        return [];
    });
    if (parts.length)
        return parts.join('\n');
    if (result['structuredContent'] !== undefined)
        return JSON.stringify(result['structuredContent'], null, 2);
    return '(MCP tool returned no text)';
}
export class ReaMcpClient {
    child;
    pending = new Map();
    catalog = [];
    nextId = 1;
    closed = false;
    constructor(child) {
        this.child = child;
        const lines = createInterface({ input: child.stdout });
        lines.on('line', line => this.receive(line));
        child.on('error', error => this.failAll(error));
        child.on('exit', (code, signal) => {
            this.closed = true;
            this.failAll(new Error(`REA MCP server exited (${signal ?? code ?? 'unknown'})`));
        });
    }
    static async start(timeoutMs = 20_000) {
        const executable = process.env['WARHEAD_REA_EXECUTABLE']?.trim() || 'rea-agents';
        const child = spawn(executable, ['mcp'], {
            cwd: process.cwd(),
            stdio: ['pipe', 'pipe', 'pipe'],
            env: { ...process.env, REA_MCP_TRANSPORT: 'stdio' },
        });
        const client = new ReaMcpClient(child);
        child.stderr.on('data', chunk => process.stderr.write(chunk));
        const initialized = await client.request('initialize', {
            protocolVersion: '2025-03-26',
            capabilities: {},
            clientInfo: { name: 'warhead', version: '0.1.0' },
        }, timeoutMs);
        if (!asObject(initialized).protocolVersion) {
            await client.close();
            throw new Error('REA MCP server returned an invalid initialize response');
        }
        client.notify('notifications/initialized');
        return client;
    }
    async tools() {
        const result = asObject(await this.request('tools/list', {}));
        const items = Array.isArray(result['tools']) ? result['tools'] : [];
        this.catalog = items.filter(item => item && typeof item.name === 'string' && item.name.trim() !== '');
        return [
            {
                def: {
                    name: 'rea_search_tasks',
                    description: `Search the self-hosted REA MCP task catalog (${this.catalog.length} tasks). Search first; the result includes exact task names and input schemas.`,
                    parameters: {
                        type: 'object',
                        properties: {
                            query: { type: 'string', description: 'Task name or analyst goal keywords.' },
                            limit: { type: 'integer', minimum: 1, maximum: 8, description: 'Maximum matching task definitions to return; defaults to 5.' },
                        },
                        required: ['query'],
                        additionalProperties: false,
                    },
                },
                mutates: false,
                run: async (args) => this.search(args),
            },
            {
                def: {
                    name: 'rea_run_task',
                    description: 'Run one task from the self-hosted REA MCP catalog. First call rea_search_tasks, then pass its exact task name and arguments. This task may inspect or change external targets and requires approval.',
                    parameters: {
                        type: 'object',
                        properties: {
                            task: { type: 'string', description: 'Exact task name returned by rea_search_tasks.' },
                            arguments: { type: 'object', description: 'Arguments matching that task’s returned input schema.' },
                        },
                        required: ['task', 'arguments'],
                        additionalProperties: false,
                    },
                },
                // The catalog spans read-only analysis through execution and mutation;
                // without a complete side-effect contract per task, fail closed.
                mutates: true,
                run: async (args, ctx) => this.runTask(args, ctx.signal),
            },
        ];
    }
    search(args) {
        const rawQuery = args['query'];
        if (typeof rawQuery !== 'string')
            return { ok: false, output: 'query must be a string', error: 'invalid query' };
        const query = rawQuery.trim().toLowerCase();
        const terms = query.split(/[^a-z0-9_]+/).filter(Boolean);
        const limit = typeof args['limit'] === 'number' && Number.isInteger(args['limit'])
            ? Math.max(1, Math.min(8, args['limit']))
            : 5;
        const ranked = this.catalog.map(tool => {
            const name = tool.name.toLowerCase();
            const description = (tool.description ?? '').toLowerCase();
            const haystack = `${name} ${description}`;
            const score = terms.reduce((total, term) => total + (name.includes(term) ? 4 : haystack.includes(term) ? 1 : 0), 0);
            return { tool, score };
        }).filter(row => terms.length === 0 || row.score > 0)
            .sort((a, b) => b.score - a.score || a.tool.name.localeCompare(b.tool.name));
        const selected = [];
        let size = 0;
        for (const { tool } of ranked) {
            if (selected.length >= limit)
                break;
            const entry = {
                name: tool.name,
                description: tool.description ?? tool.name,
                inputSchema: tool.inputSchema ?? { type: 'object', properties: {} },
            };
            const entrySize = JSON.stringify(entry).length;
            if (selected.length > 0 && size + entrySize > 24_000)
                break;
            selected.push(entry);
            size += entrySize;
        }
        return {
            ok: true,
            output: JSON.stringify({ totalTasks: this.catalog.length, matches: ranked.length, tasks: selected }, null, 2),
        };
    }
    async runTask(args, signal) {
        const task = args['task'];
        const input = args['arguments'];
        if (typeof task !== 'string' || !this.catalog.some(item => item.name === task)) {
            return { ok: false, output: 'task must exactly match a name returned by rea_search_tasks', error: 'unknown task' };
        }
        if (typeof input !== 'object' || input === null || Array.isArray(input)) {
            return { ok: false, output: 'arguments must be a JSON object matching the selected task schema', error: 'invalid arguments' };
        }
        const response = asObject(await this.request('tools/call', {
            name: task,
            arguments: input,
        }, 120_000, signal));
        const output = responseText(response);
        return response['isError'] === true
            ? { ok: false, output, error: output }
            : { ok: true, output };
    }
    async close() {
        if (this.closed)
            return;
        this.notify('exit');
        this.closed = true;
        if (this.child.exitCode === null && this.child.signalCode === null) {
            this.child.kill('SIGTERM');
            await new Promise(resolve => {
                const timer = setTimeout(() => { this.child.kill('SIGKILL'); resolve(); }, 2_000);
                this.child.once('exit', () => { clearTimeout(timer); resolve(); });
            });
        }
        this.failAll(new Error('REA MCP connection closed'));
    }
    receive(line) {
        let message;
        try {
            message = JSON.parse(line);
        }
        catch {
            return;
        }
        const id = message['id'];
        if (typeof id !== 'number')
            return;
        const pending = this.pending.get(id);
        if (!pending)
            return;
        this.pending.delete(id);
        clearTimeout(pending.timer);
        const error = asObject(message['error']);
        if (Object.keys(error).length)
            pending.reject(new Error(String(error['message'] ?? 'MCP request failed')));
        else
            pending.resolve(message['result']);
    }
    request(method, params, timeoutMs = 30_000, signal) {
        if (this.closed || !this.child.stdin.writable)
            return Promise.reject(new Error('REA MCP server is not running'));
        if (signal?.aborted)
            return Promise.reject(new Error('MCP request cancelled'));
        const id = this.nextId++;
        return new Promise((resolve, reject) => {
            const finish = (error) => {
                const entry = this.pending.get(id);
                if (!entry)
                    return;
                this.pending.delete(id);
                clearTimeout(entry.timer);
                signal?.removeEventListener('abort', abort);
                if (error)
                    reject(error);
            };
            const abort = () => {
                this.notify('notifications/cancelled', { requestId: id, reason: 'Warhead request cancelled' });
                finish(new Error('MCP request cancelled'));
            };
            const timer = setTimeout(() => {
                this.notify('notifications/cancelled', { requestId: id, reason: 'Warhead request timed out' });
                finish(new Error(`MCP ${method} timed out after ${timeoutMs}ms`));
            }, timeoutMs);
            this.pending.set(id, {
                resolve: value => { signal?.removeEventListener('abort', abort); resolve(value); },
                reject: error => { signal?.removeEventListener('abort', abort); reject(error); },
                timer,
            });
            signal?.addEventListener('abort', abort, { once: true });
            this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`, error => {
                if (error)
                    finish(error);
            });
        });
    }
    notify(method, params) {
        if (!this.closed && this.child.stdin.writable) {
            this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method, ...(params ? { params } : {}) })}\n`);
        }
    }
    failAll(error) {
        for (const [id, pending] of this.pending) {
            this.pending.delete(id);
            clearTimeout(pending.timer);
            pending.reject(error);
        }
    }
}
