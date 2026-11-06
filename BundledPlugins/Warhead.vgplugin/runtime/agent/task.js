/** Non-interactive governed entrypoint for Core workers.
 *
 * This is intentionally a facade over the canonical runTurn implementation,
 * provider selection and built-in registry. It does not copy the agent loop or
 * permission gate. A worker supplies an explicit workspace root and tool grant;
 * arbitrary shell, network and MCP capabilities are not available here.
 */
import { realpathSync, statSync } from 'node:fs';
import { isAbsolute, relative, resolve, sep } from 'node:path';
import { loadConfig } from '../config.js';
import { applyConfiguredProvider } from '../model/provider.js';
import { resolveModel } from '../model/select.js';
import { Registry } from '../tools/registry.js';
import { createTodoStore } from '../tools/todo.js';
import { runTurn } from './loop.js';
const GOVERNED_TOOLS = new Set(['read_file', 'glob', 'grep', 'todo', 'write', 'edit', 'multi_edit']);
export function governedRegistry(names) {
    if (!Array.isArray(names) || names.length < 1 || names.length > GOVERNED_TOOLS.size ||
        names.some(name => typeof name !== 'string' || !GOVERNED_TOOLS.has(name)) ||
        new Set(names).size !== names.length)
        throw new TypeError('invalid governed tool grant');
    const available = new Registry().tools;
    const selected = names.map(name => available.find(tool => tool.def.name === name));
    if (selected.some(tool => !tool))
        throw new Error('governed tool is unavailable in the shared engine');
    return new Registry(selected);
}
export async function runGovernedTask(options) {
    if (!['team', 'mgca'].includes(options.target) || typeof options.instruction !== 'string' ||
        !options.instruction.trim() || options.instruction.length > 10_000 ||
        !Number.isSafeInteger(options.maxOutputTokens) || options.maxOutputTokens < 1 ||
        options.maxOutputTokens > 1_000_000)
        throw new TypeError('invalid governed task');
    const workspaceRoot = realpathSync(options.workspaceRoot);
    if (!statSync(workspaceRoot).isDirectory())
        throw new TypeError('workspace root must be a directory');
    const registry = governedRegistry(options.allowedTools);
    const config = structuredClone(loadConfig());
    applyConfiguredProvider(config);
    if (options.target === 'team') {
        if (!config.team.enabled || config.team.members.length === 0) {
            throw new Error('Team worker requires an enabled Team profile with at least one advisor');
        }
    }
    else {
        // An MGCA claim must not silently become Team work because the operator's
        // ordinary interactive profile happened to leave advisors enabled.
        config.team.enabled = false;
    }
    if (options.model)
        config.model.candidates = [options.model];
    const resolved = await resolveModel(config, { persist: false });
    const usage = { promptTokens: 0, outputTokens: 0 };
    const errors = [];
    const mutations = new Map();
    const artifactPaths = new Set();
    const result = await runTurn({
        model: resolved.model,
        registry,
        config,
        ctx: { cwd: workspaceRoot, workspaceRoot, signal: options.signal,
            todos: createTodoStore(), config: () => config },
        history: [],
        userText: options.instruction,
        signal: options.signal,
        maxOutputTokens: options.maxOutputTokens,
        onToolStart: async (call) => options.allowedTools.includes(call.name),
        onEvent: event => {
            if (event.type === 'tool_start' && ['write', 'edit', 'multi_edit'].includes(event.name)) {
                const candidate = event.args['path'];
                if (typeof candidate === 'string' && candidate.length > 0)
                    mutations.set(event.id, { name: event.name, path: candidate });
            }
            else if (event.type === 'tool_end') {
                const mutation = mutations.get(event.id);
                mutations.delete(event.id);
                if (mutation && event.outcome.ok === true) {
                    const candidate = resolve(workspaceRoot, mutation.path);
                    const rel = relative(workspaceRoot, candidate);
                    if (rel && rel !== '..' && !rel.startsWith('..' + sep) && !isAbsolute(rel))
                        artifactPaths.add(rel);
                }
            }
            if (event.type === 'usage') {
                usage.promptTokens += Math.max(0, event.promptEvalCount);
                usage.outputTokens += Math.max(0, event.evalCount);
            }
            else if (event.type === 'error')
                errors.push(event.message);
            options.onEvent?.(event);
        },
    });
    const final = result.messages.findLast(message => message.role === 'assistant' && !message.toolCalls?.length)?.content.trim();
    const summary = (final || errors.at(-1) || `Task stopped: ${result.stopReason}`).slice(0, 10_000);
    return {
        outcome: options.signal.aborted || result.stopReason === 'cancelled' ? 'cancelled'
            : result.stopReason === 'answered' ? 'completed' : 'failed',
        summary,
        stopReason: result.stopReason,
        steps: result.steps,
        model: resolved.model,
        usage,
        artifactPaths: [...artifactPaths].sort(),
    };
}
