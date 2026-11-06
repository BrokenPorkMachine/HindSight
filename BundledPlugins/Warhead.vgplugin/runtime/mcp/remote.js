/**
 * Turn configured remote MCP servers into Warhead tools.
 */
import { HttpMcpClient } from './client.js';
export class RemoteMcpHost {
    tools = [];
    statuses = [];
    clients = [];
    constructor() { }
    static async start(servers, opts = {}) {
        const host = new RemoteMcpHost();
        const enabled = servers.filter((s) => s.enabled);
        const results = await Promise.allSettled(enabled.map(async (server) => {
            const client = await HttpMcpClient.connect(server, { fetchImpl: opts.fetchImpl, timeoutMs: opts.timeoutMs ?? 15000 });
            try {
                const tools = await client.listTools();
                return { server, client, tools };
            }
            catch (error) {
                await client.close();
                throw error;
            }
        }));
        const connected = [];
        for (let i = 0; i < enabled.length; i++) {
            const server = enabled[i];
            const res = results[i];
            if (!res)
                continue;
            if (res.status === 'fulfilled') {
                connected.push(res.value);
                host.clients.push(res.value.client);
                host.statuses.push({ name: server.name, url: server.url, ok: true, toolCount: res.value.tools.length });
            }
            else {
                const err = res.reason instanceof Error ? res.reason.message : String(res.reason);
                host.statuses.push({ name: server.name, url: server.url, ok: false, toolCount: 0, error: err });
            }
        }
        if (connected.length > 0) {
            host.tools = buildRemoteTools(connected, opts.callTimeoutMs ?? 120000);
        }
        return host;
    }
    async close() {
        await Promise.allSettled(this.clients.map((c) => c.close()));
        this.clients = [];
        this.tools = [];
    }
}
function normalizeQuery(q) {
    return q
        .toLowerCase()
        .split(/[^a-z0-9_]+/)
        .filter((x) => x.length > 0);
}
function buildRemoteTools(connected, callTimeoutMs) {
    const mutates = connected.some((c) => c.server.readOnly !== true);
    const totalTools = connected.reduce((n, c) => n + c.tools.length, 0);
    const serverNames = connected.map((c) => c.server.name).join(', ');
    const tools = [];
    tools.push({
        def: {
            name: 'mcp_search_tools',
            description: `Search tools on connected MCP servers (${serverNames}; ${totalTools} tool(s)). Returns matching tools with exact server, tool names, and input schemas. Call mcp_call_tool with the exact values returned.`,
            parameters: {
                type: 'object',
                properties: {
                    query: { type: 'string', description: 'Tool name or goal keywords to search for.' },
                    server: { type: 'string', description: 'Optional: limit search to a specific server name.' },
                    limit: { type: 'integer', minimum: 1, maximum: 8, description: 'Max results to return (default 5).' },
                },
                required: ['query'],
                additionalProperties: false,
            },
        },
        mutates: false,
        run: async (args) => {
            const rawQuery = args['query'];
            if (typeof rawQuery !== 'string')
                return { ok: false, output: 'query must be a string', error: 'invalid query' };
            const query = rawQuery.trim();
            const serverFilter = typeof args['server'] === 'string' && args['server'].trim() !== '' ? args['server'].trim().toLowerCase() : null;
            const limit = typeof args['limit'] === 'number' && Number.isInteger(args['limit']) ? Math.max(1, Math.min(8, args['limit'])) : 5;
            const terms = normalizeQuery(query);
            const ranked = [];
            for (const conn of connected) {
                if (serverFilter && conn.server.name.toLowerCase() !== serverFilter)
                    continue;
                for (const tool of conn.tools) {
                    const name = tool.name.toLowerCase();
                    const desc = (tool.description ?? '').toLowerCase();
                    const haystack = `${name} ${desc}`;
                    let score = 0;
                    if (terms.length === 0) {
                        score = 1;
                    }
                    else {
                        for (const term of terms) {
                            if (name.includes(term))
                                score += 4;
                            else if (haystack.includes(term))
                                score += 1;
                        }
                    }
                    if (terms.length > 0 && score === 0)
                        continue;
                    ranked.push({ server: conn.server.name, tool, score });
                }
            }
            ranked.sort((a, b) => b.score - a.score || a.server.localeCompare(b.server) || a.tool.name.localeCompare(b.tool.name));
            const selected = [];
            let size = 0;
            for (const row of ranked) {
                if (selected.length >= limit)
                    break;
                const entry = {
                    server: row.server,
                    name: row.tool.name,
                    description: row.tool.description ?? row.tool.name,
                    inputSchema: row.tool.inputSchema ?? { type: 'object', properties: {} },
                };
                const s = JSON.stringify(entry).length;
                if (selected.length > 0 && size + s > 24000)
                    break;
                selected.push(entry);
                size += s;
            }
            const out = {
                totalTools,
                matches: ranked.length,
                servers: connected.map((c) => c.server.name),
                tools: selected,
            };
            return { ok: true, output: JSON.stringify(out, null, 2) };
        },
    });
    tools.push({
        def: {
            name: 'mcp_call_tool',
            description: `Call a tool on a connected MCP server. Use exact server and tool names from mcp_search_tools results with arguments matching that tool's inputSchema. This may require approval depending on server configuration.`,
            parameters: {
                type: 'object',
                properties: {
                    server: { type: 'string', description: 'Exact server name from search results.' },
                    tool: { type: 'string', description: 'Exact tool name from search results.' },
                    arguments: { type: 'object', description: 'Arguments matching the tool schema.' },
                },
                required: ['server', 'tool', 'arguments'],
                additionalProperties: false,
            },
        },
        mutates,
        run: async (args, ctx) => {
            const serverName = typeof args['server'] === 'string' ? args['server'] : '';
            const toolName = typeof args['tool'] === 'string' ? args['tool'] : '';
            const input = args['arguments'];
            const conn = connected.find((c) => c.server.name === serverName);
            if (!conn) {
                return { ok: false, output: `unknown server '${serverName}'. Connected: ${connected.map((c) => c.server.name).join(', ')}`, error: 'unknown server' };
            }
            if (!conn.tools.some((t) => t.name === toolName)) {
                return { ok: false, output: `unknown tool '${toolName}' on server '${serverName}'. Run mcp_search_tools again.`, error: 'unknown tool' };
            }
            if (typeof input !== 'object' || input === null || Array.isArray(input)) {
                return { ok: false, output: 'arguments must be a JSON object matching the selected tool schema', error: 'invalid arguments' };
            }
            try {
                const res = await conn.client.callTool(toolName, input, ctx.signal, callTimeoutMs);
                if (!res.ok) {
                    return { ok: false, output: res.output, error: res.error ?? res.output };
                }
                return { ok: true, output: res.output };
            }
            catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                return { ok: false, output: msg, error: msg };
            }
        },
    });
    return tools;
}
