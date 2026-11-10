/**
 * Tool registry — declaration, dispatch, and result shaping.
 *
 * Dispatch never throws. A failing tool returns its error text *to the model*
 * so it can adapt, while the user sees the error separately. The distinction
 * matters: a crash teaches the model nothing, an error string lets it retry
 * with different arguments.
 */
import { existsSync, realpathSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve } from 'node:path';
import { readFile } from './read.js';
import { glob } from './glob.js';
import { grep } from './grep.js';
import { fastSearch } from './fast_search.js';
import { compressFile } from './compress_file.js';
import { todo } from './todo.js';
import { writeFile } from './write.js';
import { editFile } from './edit.js';
import { multiEdit } from './multi_edit.js';
import { bash } from './bash.js';
import { hindsightContext } from './hindsight.js';
import { webSearch } from './web_search.js';
import { fetchWebPage } from './web_fetch.js';
import { toolCache } from './cache.js';
/** Every tool the agent knows about. Availability is NOT decided here — the
 *  permission gate authorizes each call. This list is the capability surface;
 *  the gate is the policy surface, and keeping them separate means adding a tool
 *  can never silently widen access. */
const ENABLED = [
    readFile,
    glob,
    grep,
    fastSearch,
    compressFile,
    todo,
    writeFile,
    editFile,
    multiEdit,
    bash,
    webSearch,
    fetchWebPage,
    hindsightContext,
];
const PATH_TOOLS = new Set(['read_file', 'glob', 'grep', 'write', 'edit', 'multi_edit', 'fast_search']);
function inside(root, candidate) {
    const rel = relative(root, candidate);
    return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
}
/** Resolve an existing path or its nearest existing ancestor. This catches an
 * existing symlink and also parent-directory symlinks for a file that has not
 * been created yet. Lexical `..` segments are normalized before comparison. */
export function workspacePathAllowed(root, cwd, rawPath = '.') {
    try {
        const realRoot = realpathSync(root);
        const realCwd = realpathSync(cwd);
        if (!inside(realRoot, realCwd))
            return false;
        const requested = resolve(realCwd, rawPath);
        let ancestor = requested;
        while (!existsSync(ancestor)) {
            const parent = dirname(ancestor);
            if (parent === ancestor)
                return false;
            ancestor = parent;
        }
        const resolvedAncestor = realpathSync(ancestor);
        const resolved = resolve(resolvedAncestor, relative(ancestor, requested));
        return inside(realRoot, resolved);
    }
    catch {
        return false;
    }
}
function workspaceGuard(name, args, ctx) {
    if (!ctx.workspaceRoot)
        return undefined;
    if (name === 'compress_file') {
        for (const key of ['input', 'output']) {
            const value = args[key];
            if (typeof value !== 'string' || !workspacePathAllowed(ctx.workspaceRoot, ctx.cwd, value))
                return `Path is outside the authorized workspace: ${key}`;
        }
        return undefined;
    }
    if (!PATH_TOOLS.has(name))
        return undefined;
    const raw = args['path'];
    if (raw !== undefined && typeof raw !== 'string')
        return 'workspace path must be a string';
    if (!workspacePathAllowed(ctx.workspaceRoot, ctx.cwd, raw ?? '.')) {
        return `Path is outside the authorized workspace: ${typeof raw === 'string' ? raw : '.'}`;
    }
    return undefined;
}
export class Registry {
    tools;
    constructor(tools = ENABLED) {
        this.tools = [...tools];
        // Ensure stable, deduplicated tool list
        const seen = new Set();
        this.tools = this.tools.filter(t => {
            const name = t.def.name;
            if (seen.has(name))
                return false;
            seen.add(name);
            return true;
        });
    }
    /** Add a runtime-provided capability (for example a tool discovered from a
     * managed MCP server). Name collisions are rejected rather than silently
     * replacing a built-in tool with a different implementation. */
    register(tool) {
        if (this.get(tool.def.name))
            throw new Error(`tool already registered: ${tool.def.name}`);
        this.tools.push(tool);
    }
    names() {
        return this.tools.map(t => t.def.name);
    }
    /** Flat tool definitions. The client wraps these into the {type:'function'}
     *  envelope that /api/chat expects, so the wire shape lives in one place. */
    definitions() {
        return this.tools.map(t => t.def);
    }
    get(name) {
        return this.tools.find(t => t.def.name === name);
    }
    async dispatch(name, args, ctx) {
        const tool = this.get(name);
        if (!tool) {
            const known = this.names().join(', ');
            return {
                name,
                ok: false,
                output: `Unknown tool '${name}'. Available tools: ${known}.`,
                error: `unknown tool: ${name}`,
            };
        }
        // -------------------------------------------------------------
        // 1️⃣ Read‑only cache lookup (only for tools that are NOT mutating)
        // -------------------------------------------------------------
        // Filesystem observations must not be reused across workspaces or external edits.
        // The process-wide cache is safe only for Warhead-owned built-ins with
        // known result semantics. Custom registry tools can be scoped to a
        // workspace/task or observe external state, so their name and arguments
        // alone are not a sufficient cache key.
        const isReadOnly = ENABLED.includes(tool) && !tool.mutates && !['hindsight_context', 'fast_search', 'grep', 'glob', 'read_file'].includes(name);
        if (isReadOnly) {
            const cached = toolCache.get(name, args);
            if (cached) {
                // Return a shallow copy with the proper name field.
                return { ...cached, name };
            }
        }
        if (ctx.signal.aborted) {
            return { name, ok: false, output: 'Cancelled before the tool ran.', error: 'cancelled' };
        }
        const denied = workspaceGuard(name, args, ctx);
        if (denied)
            return { name, ok: false, output: denied, error: 'workspace scope denied' };
        try {
            const r = await tool.run(args, ctx);
            if (tool.mutates) {
                toolCache.clear();
            }
            // -------------------------------------------------------------
            // 2️⃣ Store successful read‑only results in the cache.
            // -------------------------------------------------------------
            if (isReadOnly)
                toolCache.set(name, args, r);
            return { ...r, name };
        }
        catch (err) {
            // A throw here is a bug in the tool, not a model error. Report it as a
            // tool result so the loop continues, and keep the message for the user.
            const msg = err instanceof Error ? err.message : String(err);
            return { name, ok: false, output: `Tool '${name}' failed: ${msg}`, error: msg };
        }
    }
}
export const registry = new Registry();
