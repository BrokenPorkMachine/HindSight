/**
 * Model metadata.
 *
 * A model can carry its own default system prompt in its Modelfile — for
 * gpt-oss that is the adaptive quality loop which governs how it chooses
 * reasoning depth. Sending our own `system` message REPLACES the Modelfile
 * default, so silently dropping it discards tuned behaviour the model was
 * shipped with.
 *
 * Availability is uneven: pulled models expose it via /api/show, while cloud tags
 * (`gpt-oss:120b-cloud`) return an empty string because the prompt lives
 * server-side. So this is best-effort — callers must handle null.
 *
 * Cached per model: this is metadata, not conversation state, and it costs an
 * HTTP round trip.
 */
import { providerForModel, authHeaders } from './provider.js';
import { inferenceFetch } from './inference-transport.js';
import { resolveLocalModel } from './local-models.js';
import { isVllmModel } from './vllm.js';
import { isOpenCodeModel } from './opencode.js';
const CACHE_TTL_MS = 5 * 60 * 1000;
const cache = new Map();
const pending = new Map();
async function waitForMetadata(request, signal) {
    if (!signal)
        return request;
    if (signal.aborted)
        return null;
    return new Promise(resolve => {
        const cancel = () => { signal.removeEventListener('abort', cancel); resolve(null); };
        signal.addEventListener('abort', cancel, { once: true });
        void request.then(value => {
            signal.removeEventListener('abort', cancel);
            resolve(value);
        });
    });
}
/** Share /api/show across metadata consumers, including concurrent requests.
 * Only successful responses are cached so a temporary outage can recover. */
async function fetchMetadata(model) {
    const provider = providerForModel(model);
    const key = JSON.stringify([provider.host, provider.apiKey ?? '', model]);
    const hit = cache.get(key);
    if (hit && Date.now() - hit.ts < CACHE_TTL_MS)
        return hit.value;
    const existing = pending.get(key);
    if (existing)
        return existing;
    const request = (async () => {
        try {
            const res = await inferenceFetch(`${provider.host}/api/show`, {
                method: 'POST',
                headers: { 'content-type': 'application/json', ...authHeaders(provider) },
                body: JSON.stringify({ model }),
                signal: AbortSignal.timeout(20_000),
            });
            if (!res.ok) {
                await res.body?.cancel();
                return null;
            }
            const body = await res.json();
            if (typeof body !== 'object' || body === null || Array.isArray(body))
                return null;
            return body;
        }
        catch {
            return null; // metadata is optional; never fail a turn over it
        }
    })();
    pending.set(key, request);
    try {
        const body = await request;
        // A cache clear during a request must not be undone by its completion.
        if (body && pending.get(key) === request)
            cache.set(key, { value: body, ts: Date.now() });
        return body;
    }
    finally {
        if (pending.get(key) === request)
            pending.delete(key);
    }
}
/** The model's own default system prompt, or null when it has none/is hidden. */
export async function fetchModelSystemPrompt(model, signal) {
    if (signal?.aborted)
        return null;
    if (isVllmModel(model) || isOpenCodeModel(model))
        return null;
    const local = resolveLocalModel(model);
    if (local)
        return local.engine === 'gguf' ? local.systemPrompt ?? null : null;
    const body = await waitForMetadata(fetchMetadata(model), signal);
    return typeof body?.system === 'string' ? body.system.trim() || null : null;
}
/**
 * Compose the system message: model defaults FIRST (preserved from Modelfile),
 * then minimal agent framing. Design goal: model behaves as pure Ollama
 * (reasoning protocols intact), with agent behaviors (max_steps, repetition
 * suppression, gate, compaction) enforced by code — not embedded as
 * instruction prose in the prompt.
 *
 * The agent block is kept to cwd, tool names, mutation policy, and the
 * code-enforced constraint note. No brand/personal framing; no hidden
 * reasoning instructions. Tool usage rules live in loop.ts, not here.
 */
export function composeSystemPrompt(modelDefault, agentPrompt) {
    const base = modelDefault?.trim();
    if (!base)
        return agentPrompt;
    return [
        '# Model defaults',
        '',
        base,
        '',
        '# Agent instructions',
        '',
        'The above are your built-in defaults and remain in effect. The following '
            + 'define your role for this session and take precedence where they conflict.',
        '',
        agentPrompt,
    ].join('\n');
}
/** Extract a model's context window (in tokens) from an /api/show body.
 *
 *  Ollama reports the maximum context length in `model_info` under an
 *  architecture-specific key (e.g. `llama.context_length`), and a configured
 *  `num_ctx` may also appear in the `parameters` block. Cloud tags expose the
 *  same metadata, so this works for both local and remote models. */
export function extractContextLength(body) {
    if (typeof body !== 'object' || body === null)
        return null;
    const b = body;
    if (typeof b['context_length'] === 'number' && Number.isFinite(b['context_length']) && b['context_length'] >= 1) {
        return Math.trunc(b['context_length']);
    }
    const info = b['model_info'];
    if (typeof info === 'object' && info !== null) {
        for (const [k, v] of Object.entries(info)) {
            if (k.endsWith('.context_length') || k === 'context_length') {
                if (typeof v === 'number' && Number.isFinite(v) && v >= 1)
                    return Math.trunc(v);
            }
        }
    }
    // Modelfile `PARAMETER num_ctx N` surfaces in the parameters string.
    const params = b['parameters'];
    if (typeof params === 'string') {
        const m = params.match(/(^|\s)num_ctx\s+(\d+)/m);
        if (m) {
            const value = Number(m[2]);
            if (Number.isSafeInteger(value) && value >= 1)
                return value;
        }
    }
    // Some builds nest it under details.
    const details = b['details'];
    if (typeof details === 'object' && details !== null) {
        const d = details;
        if (typeof d['context_length'] === 'number' && Number.isFinite(d['context_length']) && d['context_length'] >= 1)
            return Math.trunc(d['context_length']);
    }
    return null;
}
/** Allocate enough context for this request without reserving the model's
 * entire advertised maximum. A 128k context on a small local model can exhaust
 * the Ollama host before it sends a response. */
export const MAX_AUTO_CONTEXT_TOKENS = 32_768;
export function requestContextLength(advertised, requestChars) {
    const estimated = Math.ceil(requestChars / 4) + 2048; // generation headroom
    const wanted = Math.max(8192, Math.ceil(estimated / 4096) * 4096);
    return Math.min(wanted, advertised ?? MAX_AUTO_CONTEXT_TOKENS, MAX_AUTO_CONTEXT_TOKENS);
}
/** The model's context window in tokens, or null when it cannot be determined. */
export async function fetchModelContextLength(model, signal) {
    if (signal?.aborted)
        return null;
    if (isVllmModel(model) || isOpenCodeModel(model))
        return null; // Offline vLLM has no Ollama /api/show.
    // Match the native M2 GGUF context rather than advertising the GGUF's
    // theoretical training length; prevents repeated prompt-overflow failures.
    try {
        const local = resolveLocalModel(model);
        if (local?.engine === 'gguf') {
            const configured = Number(process.env['WARHEAD_M2_CONTEXT_TOKENS']);
            return Number.isSafeInteger(configured) && configured >= 1024 && configured <= 32768 ? configured : 4096;
        }
    }
    catch { /* preserve existing local-reference validation in caller */ }
    return extractContextLength(await waitForMetadata(fetchMetadata(model), signal));
}
// Aliases covering the names callers and tests are likely to use.
export const fetchModelContextSize = fetchModelContextLength;
export const fetchModelContextWindow = fetchModelContextLength;
export const getModelContextLength = fetchModelContextLength;
export const modelContextLength = fetchModelContextLength;
export const contextLength = fetchModelContextLength;
export function clearModelSystemPromptCache() {
    cache.clear();
    pending.clear();
}
export function refreshModelSystemPromptCache() { clearModelSystemPromptCache(); }
export function clearModelContextCache() {
    clearModelSystemPromptCache();
}
