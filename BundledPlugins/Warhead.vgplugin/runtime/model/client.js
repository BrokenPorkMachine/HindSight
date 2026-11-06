/**
 * Model chat client.
 *
 * Local `mlx:`, `gguf:` and `ollama:` references run in Warhead's native Swift
 * runtime; everything else is plain HTTP + JSON against an Ollama-compatible
 * endpoint — no SDK. HTTP streams NDJSON from /api/chat and assembles tool calls
 * across events, since a single call may be split across several stream frames.
 */
import { providerForModel, authHeaders } from './provider.js';
import { streamNativeChat } from './native-runtime.js';
import { isVllmModel, streamVllmChat, vllmChatOnce } from './vllm.js';
import { resolveLocalModel } from './local-models.js';
import { isOpenCodeModel, openCodeChatOnce, streamOpenCodeChat, OpenCodeError } from './opencode.js';
import { Agent } from 'undici';
import { inferenceDispatcher, inferenceFetch } from './inference-transport.js';
const DEFAULT_CHAT_IDLE_TIMEOUT_MS = 15 * 60 * 1000;
class OllamaAgent extends Agent {
    dispatch(options, handler) {
        const configured = Number(process.env['WARHEAD_OLLAMA_IDLE_TIMEOUT_MS']);
        const timeout = Number.isSafeInteger(configured) && configured > 0 && configured <= 2_147_483_647
            ? configured : DEFAULT_CHAT_IDLE_TIMEOUT_MS;
        // fetch sets its own five-minute limits. Override at dispatch time so a
        // CPU server can finish evaluating a long prompt before its first token.
        return super.dispatch({ ...options, headersTimeout: timeout, bodyTimeout: timeout }, handler);
    }
}
const ollamaAgent = new OllamaAgent();
const localCache = new Map();
function cachedResolve(model) {
    if (!localCache.has(model))
        localCache.set(model, resolveLocalModel(model));
    return localCache.get(model);
}
export class OllamaError extends Error {
    // Declared explicitly: parameter properties are not erasable syntax and are
    // rejected by Node's type-stripping loader.
    cause;
    /** HTTP status when the failure came from the daemon, else undefined. */
    status;
    /** True when retrying the same request could plausibly succeed. */
    transient;
    constructor(message, cause, status, retryable = false) {
        super(message);
        this.name = 'OllamaError';
        this.cause = cause;
        this.status = status;
        // 5xx and 429 are the daemon or an upstream proxy failing, not a bad
        // request. Observed in practice: Ollama's cloud path returns
        // "Internal Server Error (ref: ...)" mid-stream, which is worth one retry.
        this.transient = retryable || status === 429 || (status !== undefined && status >= 500);
    }
}
export function isTransient(err) {
    if (err instanceof OllamaError || err instanceof OpenCodeError)
        return err.transient;
    // Connection-level failures (daemon restarting, network blip).
    if (err instanceof Error && (err.name === 'TypeError' || err.name === 'FetchError'))
        return true;
    return false;
}
function connectionError(host, cause) {
    const codes = new Set();
    const inspect = (error, depth = 0) => {
        if (!error || typeof error !== 'object' || depth > 4)
            return;
        const details = error;
        if (typeof details.code === 'string' && /^[A-Z][A-Z0-9_]{1,50}$/.test(details.code))
            codes.add(details.code);
        inspect(details.cause, depth + 1);
        if (Array.isArray(details.errors))
            for (const nested of details.errors.slice(0, 8))
                inspect(nested, depth + 1);
    };
    inspect(cause);
    const detail = codes.size ? ` (${[...codes].join(', ')})` : '';
    const message = codes.has('UND_ERR_HEADERS_TIMEOUT') || codes.has('UND_ERR_BODY_TIMEOUT')
        ? `Ollama at ${host} timed out waiting for model output${detail}`
        : `cannot reach Ollama at ${host}${detail}`;
    return new OllamaError(message, cause, undefined, true);
}
/** Ollama emits `arguments` as an object for some models and a JSON string for
 *  others. Normalize to an object so tool code never branches on this. */
export function parseArgs(raw) {
    if (raw == null)
        return {};
    if (typeof raw === 'string') {
        const trimmed = raw.trim();
        if (trimmed === '')
            return {};
        try {
            const parsed = JSON.parse(trimmed);
            return typeof parsed === 'object' && parsed !== null
                ? parsed
                : { value: parsed };
        }
        catch {
            return { _unparsed: raw };
        }
    }
    if (typeof raw === 'object')
        return raw;
    return { value: raw };
}
/** Wrap flat definitions into the envelope /api/chat expects. */
function toWireTools(tools) {
    return tools.map(t => ({
        type: 'function',
        function: { name: t.name, description: t.description, parameters: t.parameters },
    }));
}
function parseStreamFrame(line) {
    let value;
    try {
        value = JSON.parse(line);
    }
    catch (err) {
        throw new OllamaError('Ollama returned invalid stream JSON', err);
    }
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
        throw new OllamaError('Ollama returned a non-object stream frame');
    }
    return value;
}
function* streamFrameEvents(frame, seq) {
    if (frame.error)
        throw new OllamaError(`model error: ${frame.error}`);
    const msg = frame.message;
    if (msg?.content)
        yield { type: 'text', delta: msg.content };
    for (const call of normalizeToolCalls(msg?.tool_calls, seq)) {
        yield { type: 'tool_call', call };
    }
    if (frame.done) {
        yield {
            type: 'done',
            evalCount: frame.eval_count ?? 0,
            promptEvalCount: frame.prompt_eval_count ?? 0,
        };
    }
}
function normalizeToolCalls(raw, seq) {
    if (!Array.isArray(raw))
        return [];
    const out = [];
    for (const entry of raw) {
        if (typeof entry !== 'object' || entry === null)
            continue;
        const e = entry;
        const fn = (e['function'] ?? {});
        const name = typeof fn['name'] === 'string' ? fn['name'] : undefined;
        if (!name)
            continue;
        out.push({
            id: typeof e['id'] === 'string' && e['id'] ? e['id'] : seq(),
            name,
            args: parseArgs(fn['arguments']),
        });
    }
    return out;
}
/** Ollama expects `role:'tool'` results keyed to the originating call. Our
 *  ChatMessage keeps both halves so the request builder can shape them. */
function toWire(messages) {
    return messages.map(m => {
        if (m.role === 'tool') {
            return {
                role: 'tool',
                content: m.content,
                tool_name: m.toolName ?? 'unknown',
                ...(m.toolCallId ? { tool_call_id: m.toolCallId } : {}),
            };
        }
        if (m.role === 'assistant' && m.toolCalls?.length) {
            return {
                role: 'assistant',
                content: m.content,
                tool_calls: m.toolCalls.map(c => ({
                    id: c.id,
                    function: { name: c.name, arguments: c.args },
                })),
            };
        }
        return { role: m.role, content: m.content };
    });
}
export async function* streamChat(model, messages, opts = {}) {
    if (isOpenCodeModel(model)) {
        yield* streamOpenCodeChat(model, messages, opts);
        return;
    }
    if (isVllmModel(model)) {
        yield* streamVllmChat(model, messages, opts);
        return;
    }
    const source = cachedResolve(model);
    if (source?.engine === 'mlx') {
        yield* streamNativeChat(model, messages, { tools: opts.tools, signal: opts.signal, numPredict: opts.numPredict });
        return;
    }
    if (source?.engine === 'gguf') {
        yield* streamNativeChat(`gguf:${source.path}`, messages, {
            tools: opts.tools,
            signal: opts.signal,
            numPredict: opts.numPredict,
            // Carry the Ollama manifest's template and stop markers: they describe
            // the prompt format this tag was actually built for.
            ...(source.template ? { template: source.template } : {}),
            ...(source.stops?.length ? { stops: source.stops } : {}),
        });
        return;
    }
    const body = {
        model,
        messages: toWire(messages),
        stream: true,
        // Warhead manages compaction. Reject an oversized request instead of
        // silently dropping older messages or part of a pasted attachment.
        truncate: false,
        options: {
            ...(opts.numPredict != null ? { num_predict: opts.numPredict } : {}),
            ...(opts.numCtx != null ? { num_ctx: opts.numCtx } : {}),
            ...(opts.keepAlive != null ? { keep_alive: opts.keepAlive } : {}),
        },
    };
    if (opts.tools?.length)
        body['tools'] = toWireTools(opts.tools);
    if (opts.format != null)
        body['format'] = opts.format;
    if (opts.think != null)
        body['think'] = opts.think;
    // Resolved per call, not captured at module load, so switching provider
    // mid-session takes effect on the next turn and tests can point at a fake
    // server without import-order gymnastics.
    const provider = providerForModel(model);
    let res;
    try {
        res = await inferenceFetch(`${provider.host}/api/chat`, {
            dispatcher: inferenceDispatcher ?? ollamaAgent,
            method: 'POST',
            headers: { 'content-type': 'application/json', ...authHeaders(provider) },
            body: JSON.stringify(body),
            ...(opts.signal ? { signal: opts.signal } : {}),
        });
    }
    catch (err) {
        if (err instanceof Error && err.name === 'AbortError')
            return;
        throw connectionError(provider.host, err);
    }
    if (!res.ok) {
        const detail = await res.text().catch(() => '');
        throw new OllamaError(`HTTP ${res.status} from Ollama: ${detail.slice(0, 300)}`, undefined, res.status);
    }
    if (!res.body)
        throw new OllamaError('Ollama returned an empty response body');
    let counter = 0;
    const seq = () => `call_${++counter}`;
    const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = '';
    let completed = false;
    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done)
                break;
            buffer += value;
            if (buffer.length > 2 * 1024 * 1024 && !buffer.includes('\n'))
                throw new OllamaError('Ollama stream frame exceeds the 2 MiB limit');
            let nl;
            while ((nl = buffer.indexOf('\n')) !== -1) {
                const line = buffer.slice(0, nl).trim();
                if (line.length > 2 * 1024 * 1024)
                    throw new OllamaError('Ollama stream frame exceeds the 2 MiB limit');
                buffer = buffer.slice(nl + 1);
                if (line === '')
                    continue;
                const frame = parseStreamFrame(line);
                yield* streamFrameEvents(frame, seq);
                if (frame.done)
                    completed = true;
            }
        }
        if (buffer.trim() !== '') {
            let frame;
            try {
                frame = parseStreamFrame(buffer.trim());
            }
            catch (err) {
                throw new OllamaError('Ollama stream ended with incomplete JSON', err, undefined, true);
            }
            yield* streamFrameEvents(frame, seq);
            if (frame.done)
                completed = true;
        }
        if (!completed)
            throw new OllamaError('Ollama stream ended before completion', undefined, undefined, true);
    }
    catch (err) {
        if (opts.signal?.aborted || (err instanceof Error && err.name === 'AbortError'))
            return;
        if (err instanceof OllamaError)
            throw err;
        throw connectionError(provider.host, err);
    }
    finally {
        await reader.cancel().catch(() => { });
        reader.releaseLock();
    }
}
/** Non-streaming single-turn call. Used by the capability probe, where we need
 *  a definitive answer rather than incremental output. */
export async function chatOnce(model, messages, opts = {}) {
    if (isOpenCodeModel(model))
        return openCodeChatOnce(model, messages, opts);
    if (isVllmModel(model))
        return vllmChatOnce(model, messages, opts);
    if (cachedResolve(model)) {
        let content = '';
        const toolCalls = [];
        for await (const event of streamChat(model, messages, opts)) {
            if (event.type === 'text')
                content += event.delta;
            else if (event.type === 'tool_call')
                toolCalls.push(event.call);
        }
        return { content, toolCalls };
    }
    const body = {
        model,
        messages: toWire(messages),
        stream: false,
        truncate: false,
        options: { num_predict: opts.numPredict ?? 256 },
    };
    if (opts.numCtx != null)
        body['options']['num_ctx'] = opts.numCtx;
    if (opts.keepAlive != null)
        body['options']['keep_alive'] = opts.keepAlive;
    if (opts.tools?.length)
        body['tools'] = toWireTools(opts.tools);
    if (opts.format != null)
        body['format'] = opts.format;
    if (opts.think != null)
        body['think'] = opts.think;
    const provider = providerForModel(model);
    let res;
    try {
        res = await inferenceFetch(`${provider.host}/api/chat`, {
            dispatcher: inferenceDispatcher ?? ollamaAgent,
            method: 'POST',
            headers: { 'content-type': 'application/json', ...authHeaders(provider) },
            body: JSON.stringify(body),
            ...(opts.signal ? { signal: opts.signal } : {}),
        });
    }
    catch (err) {
        if (err instanceof Error && err.name === 'AbortError')
            return { content: '', toolCalls: [] };
        throw connectionError(provider.host, err);
    }
    if (!res.ok) {
        const detail = await res.text().catch(() => '');
        throw new OllamaError(`HTTP ${res.status} from Ollama: ${detail.slice(0, 300)}`, undefined, res.status);
    }
    const frame = (await res.json());
    if (frame.error)
        throw new OllamaError(`model error: ${frame.error}`);
    let counter = 0;
    const msg = frame.message;
    return {
        content: msg?.content ?? '',
        toolCalls: normalizeToolCalls(msg?.tool_calls, () => `call_${++counter}`),
    };
}
export { fetchModelContextLength, fetchModelContextSize, fetchModelContextWindow, getModelContextLength, modelContextLength, contextLength, extractContextLength, clearModelContextCache, } from './modelfile.js';
