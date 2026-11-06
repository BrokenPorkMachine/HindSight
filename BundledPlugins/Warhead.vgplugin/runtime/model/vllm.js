/** Opt-in serverless vLLM backend. Only vllm: references enter this path.
 * vLLM runs inside a Warhead-owned, local Python worker. No HTTP listener.
 */
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
export const isVllmModel = (model) => model.startsWith('vllm:');
export class VllmError extends Error {
    constructor(message) { super(message); this.name = 'VllmError'; }
}
const moduleDir = dirname(fileURLToPath(import.meta.url));
const workerPath = join(moduleDir, 'vllm-worker.py');
let processWorker = null;
let selectedModel = null;
let sequence = 0;
let pending = new Map();
let queue = Promise.resolve();
const MAX_EVENT_BYTES = 8 * 1024 * 1024;
let modeEnabled = false;
export function enableVllmMode() { modeEnabled = true; }
export function vllmModeEnabled() { return modeEnabled; }
function stop(error) {
    const previous = processWorker;
    processWorker = null;
    selectedModel = null;
    for (const entry of pending.values())
        entry.fail(error);
    pending.clear();
    if (previous && previous.exitCode === null)
        previous.kill();
}
export async function preloadVllm(model) {
    if (!modeEnabled)
        throw new VllmError('vLLM is opt-in: start Warhead with --vllm');
    if (!isVllmModel(model) || !model.slice(5))
        throw new VllmError('Expected vllm:<model-path-or-HF-id>');
    if (processWorker && selectedModel === model && processWorker.exitCode === null && !processWorker.killed)
        return;
    if (processWorker)
        await shutdownVllm();
    const python = process.env['WARHEAD_VLLM_PYTHON'] || 'python3';
    const child = spawn(python, ['-u', workerPath], {
        stdio: ['pipe', 'pipe', 'pipe'],
        env: { ...process.env, WARHEAD_VLLM_MODEL: model.slice(5) },
    });
    processWorker = child;
    selectedModel = model;
    child.stderr.on('data', c => process.stderr.write(c));
    const ready = new Promise((resolve, reject) => {
        const deadline = setTimeout(() => settle(new VllmError('vLLM initialization exceeded 300 seconds')), 300000);
        let settled = false;
        const settle = (error) => { if (settled)
            return; settled = true; clearTimeout(deadline); if (error)
            reject(error);
        else
            resolve(); };
        child.on('error', err => settle(err));
        child.on('exit', (code, signal) => { settle(new VllmError('vLLM worker exited before ready: ' + (signal ?? code))); stop(new VllmError('vLLM worker exited')); });
        const rl = createInterface({ input: child.stdout });
        rl.on('line', line => {
            let value;
            try {
                value = JSON.parse(line);
            }
            catch {
                settle(new VllmError('Invalid worker JSON'));
                return;
            }
            if (value.id === 0 && value.event === 'ready') {
                settle();
                return;
            }
            if (value.id === 0 && value.event === 'error') {
                settle(new VllmError(value.message ?? 'vLLM initialization failed'));
                return;
            }
            const entry = pending.get(value.id);
            if (!entry)
                return;
            entry.push(value);
            if (value.event === 'error' || value.event === 'done')
                pending.delete(value.id);
        });
    });
    try {
        await ready;
    }
    catch (error) {
        stop(error instanceof Error ? error : new Error(String(error)));
        throw error;
    }
}
export async function shutdownVllm() {
    const child = processWorker;
    if (!child)
        return;
    processWorker = null;
    selectedModel = null;
    if (child.exitCode !== null)
        return;
    child.stdin.write(JSON.stringify({ id: ++sequence, method: 'shutdown' }) + '\n');
    await Promise.race([
        new Promise(resolve => child.once('exit', () => resolve())),
        new Promise(resolve => setTimeout(resolve, 2000)),
    ]);
    if (child.exitCode === null)
        child.kill('SIGTERM');
}
async function* run(model, messages, tools, numPredict, signal) {
    const prior = queue;
    let release;
    queue = new Promise(resolve => { release = resolve; });
    await prior;
    try {
        if (signal?.aborted)
            throw new VllmError('vLLM request cancelled');
        await preloadVllm(model);
        const child = processWorker;
        if (!child)
            throw new VllmError('Worker not ready');
        const id = ++sequence;
        const events = [];
        let bytes = 0;
        let wake;
        let finished = false;
        let failure;
        const fail = (error) => {
            failure = error;
            finished = true;
            wake?.();
            wake = undefined;
        };
        pending.set(id, {
            push: event => {
                if (event.event === 'error')
                    fail(new VllmError(event.message ?? 'vLLM error'));
                else {
                    bytes += Buffer.byteLength(JSON.stringify(event));
                    if (bytes > MAX_EVENT_BYTES) {
                        fail(new VllmError('vLLM event queue exceeded 8 MiB'));
                        return;
                    }
                    events.push(event);
                    if (event.event === 'done')
                        finished = true;
                    wake?.();
                    wake = undefined;
                }
            }, fail,
        });
        const cancel = (reason) => {
            if (finished)
                return;
            // Request-level cancellation is cooperative. If it fails to acknowledge,
            // stop() retires the GPU worker to prevent stuck compute.
            try {
                child.stdin.write(JSON.stringify({ id: ++sequence, method: 'cancel', target: id }) + '\n');
            }
            catch { /* exit listener handles it */ }
            const kill = setTimeout(() => stop(reason), 1500);
            kill.unref();
            fail(reason);
        };
        const onAbort = () => cancel(new VllmError('vLLM request cancelled'));
        signal?.addEventListener('abort', onAbort, { once: true });
        const timeout = setTimeout(() => cancel(new VllmError('vLLM generation timeout')), 300000);
        try {
            if (signal?.aborted)
                throw new VllmError('vLLM request cancelled');
            child.stdin.write(JSON.stringify({ id, method: 'chat', messages: messages.map(m => ({
                    role: m.role, content: m.content,
                    ...(m.toolCallId ? { tool_call_id: m.toolCallId } : {}),
                    ...(m.toolName ? { tool_name: m.toolName } : {}),
                    ...(m.toolCalls ? { tool_calls: m.toolCalls.map(c => ({ id: c.id, type: 'function', function: { name: c.name, arguments: JSON.stringify(c.args) } })) } : {}),
                })), tools: tools.map(t => ({ type: 'function', function: t })), numPredict }) + '\n');
            while (!finished || events.length) {
                if (events.length) {
                    const event = events.shift();
                    bytes -= Buffer.byteLength(JSON.stringify(event));
                    if (event.event === 'text' && event.delta)
                        yield { type: 'text', delta: event.delta };
                    if (event.event === 'tool_call' && event.call)
                        yield { type: 'tool_call', call: event.call };
                    if (event.event === 'done')
                        yield { type: 'done', evalCount: event.evalCount ?? 0, promptEvalCount: event.promptEvalCount ?? 0 };
                    continue;
                }
                if (failure)
                    throw failure;
                await new Promise(resolve => { wake = resolve; });
            }
            if (failure)
                throw failure;
        }
        finally {
            clearTimeout(timeout);
            signal?.removeEventListener('abort', onAbort);
            pending.delete(id);
            if (!finished)
                cancel(new VllmError('vLLM stream closed before completion'));
        }
    }
    finally {
        release();
    }
}
export async function* streamVllmChat(model, messages, opts = {}) {
    yield* run(model, messages, opts.tools ?? [], opts.numPredict ?? 2048, opts.signal);
}
export async function vllmChatOnce(model, messages, opts = {}) {
    let content = '';
    const toolCalls = [];
    for await (const event of streamVllmChat(model, messages, opts)) {
        if (event.type === 'text')
            content += event.delta;
        if (event.type === 'tool_call')
            toolCalls.push(event.call);
    }
    return { content, toolCalls };
}
