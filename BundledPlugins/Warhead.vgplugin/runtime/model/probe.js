/**
 * Model capability probe.
 *
 * Tool support is NOT discoverable from Ollama's metadata, and a model without
 * it answers HTTP 200 with text and no tool_calls — indistinguishable from
 * success at the transport layer. So support has to be established by asking the
 * model to call a tool and checking that it did.
 *
 * Tri-state on purpose: `unsupported` (answered, but won't call tools) must be
 * distinguishable from `unreachable` (transport failure), because the first is
 * a permanent property of the model and the second may be transient.
 */
import { chatOnce, OllamaError } from './client.js';
import { inferenceFetch } from './inference-transport.js';
import { activeProvider, authHeaders } from './provider.js';
import { listLocalNativeModels, listLocalOllamaModels, LocalModelError, resolveLocalModel } from './local-models.js';
import { prepareNativeRuntime } from './native-runtime.js';
const PROBE_TOOL = {
    name: 'read_file',
    description: 'Read a file from the local filesystem',
    parameters: {
        type: 'object',
        properties: { path: { type: 'string', description: 'absolute path' } },
        required: ['path'],
    },
};
const PROBE_MESSAGES = [
    {
        role: 'user',
        content: 'Use the read_file tool to read the file /etc/hosts. Call the tool now.',
    },
];
const DEFAULT_TIMEOUT_MS = 60_000;
const DEFAULT_ATTEMPTS = 2;
/**
 * Reasoning models (gpt-oss reports a `thinking` capability) spend generated
 * tokens on hidden reasoning before emitting content. With a small budget the
 * whole allowance can be consumed by reasoning, leaving empty content and no
 * tool call — which would be misreported as `unsupported`. Hence a budget with
 * room to spare, and a re-probe at a larger one before concluding anything.
 */
const PROBE_NUM_PREDICT = 800;
const PROBE_NUM_PREDICT_RETRY = 2000;
/** One probe attempt. Kept separate so the retry loop can classify a failure
 *  without conflating "flaky" with "unsupported". */
async function attempt(model, timeoutMs, numPredict) {
    // A first-run Swift build can take minutes. Do it before the timeout starts:
    // otherwise the timer expires the moment the synchronous build returns and the
    // probe is marked unreachable even though the runtime is now ready.
    try {
        if (resolveLocalModel(model))
            await prepareNativeRuntime();
    }
    catch (err) {
        const detail = err instanceof Error ? err.message.slice(0, 200) : String(err);
        return err instanceof LocalModelError
            ? { ok: false, status: 'unsupported', detail }
            : { ok: false, status: 'unreachable', detail };
    }
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
        const { content, toolCalls } = await chatOnce(model, PROBE_MESSAGES, {
            tools: [PROBE_TOOL],
            numPredict,
            signal: controller.signal,
        });
        if (toolCalls.length > 0)
            return { ok: true, calls: toolCalls.length };
        // Empty output with no tool call is ambiguous: it is what a reasoning model
        // looks like when it runs out of budget mid-thought. Retry larger.
        if (content.trim() === '')
            return { ok: false, status: 'retry', detail: 'empty completion' };
        return { ok: false, status: 'unsupported', detail: 'answered without emitting a tool call' };
    }
    catch (err) {
        if (err instanceof OllamaError) {
            const text = err.message;
            if (err.cause instanceof Error && err.cause.name === 'AbortError') {
                return { ok: false, status: 'unreachable', detail: `timed out after ${timeoutMs}ms` };
            }
            // A 404 means the tag does not resolve — permanent, so do not retry.
            if (/HTTP 404/.test(text)) {
                return { ok: false, status: 'unsupported', detail: 'model tag not found' };
            }
            return { ok: false, status: 'unreachable', detail: text.slice(0, 200) };
        }
        if (err instanceof Error && /unknown model architecture|unsupported model architecture|could not load GGUF model/i.test(err.message)) {
            return { ok: false, status: 'unsupported', detail: err.message.slice(0, 200) };
        }
        if (err instanceof Error && err.name === 'AbortError') {
            return { ok: false, status: 'unreachable', detail: `timed out after ${timeoutMs}ms` };
        }
        return {
            ok: false,
            status: 'unreachable',
            detail: err instanceof Error ? err.message.slice(0, 200) : String(err),
        };
    }
    finally {
        clearTimeout(timer);
    }
}
/**
 * Probe whether a model can call tools.
 *
 * Retries by default: tool-call emission is stochastic for some models, so a
 * single miss is not evidence of absence. Observed: `gpt-6-astra` produced a
 * valid call in one session and none in the next with an identical prompt,
 * which a single-shot probe would have misreported as unsupported.
 *
 * `unreachable` aborts immediately — a transport failure says nothing about
 * tool support and retrying only wastes the fallback budget.
 */
export async function probeModel(model, timeoutMs = DEFAULT_TIMEOUT_MS, attempts = DEFAULT_ATTEMPTS) {
    const started = Date.now();
    let last = {
        status: 'unreachable',
        detail: 'no attempt completed',
    };
    for (let i = 0; i < Math.max(1, attempts); i++) {
        const r = await attempt(model, timeoutMs, PROBE_NUM_PREDICT);
        if (r.ok) {
            return {
                model,
                status: 'ok',
                detail: `emitted ${r.calls} tool call(s)${i > 0 ? ` (attempt ${i + 1})` : ''}`,
                latencyMs: Date.now() - started,
            };
        }
        if (r.status === 'retry') {
            const bigger = await attempt(model, timeoutMs, PROBE_NUM_PREDICT_RETRY);
            if (bigger.ok) {
                return {
                    model,
                    status: 'ok',
                    detail: `emitted ${bigger.calls} tool call(s) (larger reasoning budget)`,
                    latencyMs: Date.now() - started,
                };
            }
            last = { status: 'unsupported', detail: 'no tool call and no output at 2000 tokens' };
            break;
        }
        last = { status: r.status, detail: r.detail };
        if (r.status === 'unreachable')
            break;
    }
    return { model, status: last.status, detail: last.detail, latencyMs: Date.now() - started };
}
/** Probe candidates in order, returning the first that works plus the full
 *  audit trail so failures can be reported rather than silently swallowed. */
export async function selectModel(candidates, timeoutMs) {
    const results = [];
    for (const model of candidates) {
        const r = await probeModel(model, timeoutMs);
        results.push(r);
        if (r.status === 'ok')
            return { chosen: model, results };
    }
    return { chosen: null, results };
}
/** Narrow a model list to the terms given, case-insensitively, matching on any
 *  term. Substring rather than glob because that is what a person typing
 *  `/model pick oss` expects, and `*` in a term is treated literally. */
export function filterModels(models, terms) {
    const wanted = terms.map(t => t.trim().toLowerCase()).filter(t => t !== '');
    if (wanted.length === 0)
        return models;
    return models.filter(m => {
        const hay = m.toLowerCase();
        return wanted.some(t => hay.includes(t));
    });
}
/** True when a tag is resolved on demand rather than held on this disk.
 *
 *  Heuristic, and deliberately cheap. `/api/tags` has no "is remote" field; the
 *  authoritative one lives on `/api/show` as `remote_model`, but asking per
 *  model would mean a request for every row of a list that is meant to be free.
 *
 *  A real pulled model weighs hundreds of MB or more, so a few-KB `size` means
 *  Ollama is reporting the size of a *manifest* it will fetch from the registry
 *  on first use. `-cloud` is matched directly. `parameter_size` being empty
 *  corroborates it, since that metadata only exists once weights are local. */
function isRemoteTag(name, entry) {
    if (/-cloud(:|$)/.test(name))
        return true;
    const size = typeof entry.size === 'number' ? entry.size : undefined;
    if (size !== undefined && size > 0 && size < 1024 * 1024)
        return true;
    if (!entry.details?.parameter_size && (size === undefined || size < 1024 * 1024))
        return true;
    return false;
}
/** Is the endpoint answering at all?
 *
 *  Separate from the tool probe on purpose. Probing candidates against a host
 *  that is not there costs one full timeout per candidate — switching to a
 *  mistyped address took ~20s to say so. /api/version is a single cheap
 *  request, so an unreachable host is reported in about a second instead. */
export async function checkReachable(provider, timeoutMs = 5_000) {
    try {
        const res = await inferenceFetch(`${provider.host}/api/version`, {
            headers: authHeaders(provider),
            signal: AbortSignal.timeout(timeoutMs),
        });
        // Any HTTP response means something answered, which is the only question
        // here. A 404 still proves the host is up — it means this build does not
        // serve /api/version. Scoring that as "unreachable" would refuse to switch
        // to a perfectly working endpoint, so the status is reported, not judged.
        const body = (await res.json().catch(() => ({})));
        return { ok: true, status: res.status, version: body.version };
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        return { ok: false, detail: /timeout|abort/i.test(msg) ? `no response within ${timeoutMs}ms` : msg };
    }
}
/** Installed models with detail: the local Ollama store plus whatever the
 *  active endpoint reports.
 *
 *  The endpoint contributes only tags it knows; Ollama exposes no cloud
 *  catalog, so cloud candidates must be configured or typed. A cloud tag still
 *  appears here once it has been used, and is flagged via `remote`. When both
 *  sources carry the same tag, the local `ollama:` reference wins: it selects
 *  Warhead-owned execution, which keeps working without the daemon. */
export async function listInstalled(p) {
    const provider = p ?? activeProvider();
    const ollamaRefs = listLocalOllamaModels();
    // Ollama-store tags come first so a first run prefers what the daemon serves;
    // loose native weights are still listed because they are otherwise invisible
    // and cannot be used without retyping an absolute path.
    const localRefs = [...ollamaRefs, ...listLocalNativeModels()];
    const localTags = new Set(ollamaRefs.map(ref => ref.slice('ollama:'.length)));
    const out = [];
    const seen = new Set();
    try {
        const res = await inferenceFetch(`${provider.host}/api/tags`, {
            headers: authHeaders(provider),
            signal: AbortSignal.timeout(5_000),
        });
        if (res.ok) {
            const body = (await res.json());
            for (const m of body.models ?? []) {
                const name = m.name ?? m.model;
                if (typeof name !== 'string' || name.length === 0 || seen.has(name))
                    continue;
                // The same tag is served by Ollama and present in the local store. Keep
                // the `ollama:` reference: it selects Warhead-owned execution, which
                // keeps working when the daemon is not running.
                if (localTags.has(name))
                    continue;
                seen.add(name);
                out.push({
                    name,
                    sizeBytes: typeof m.size === 'number' ? m.size : undefined,
                    parameterSize: m.details?.parameter_size,
                    family: m.details?.family,
                    remote: isRemoteTag(name, m),
                });
            }
        }
    }
    catch { /* local manifests remain discoverable without the Ollama daemon */ }
    for (const name of localRefs) {
        if (seen.has(name))
            continue;
        seen.add(name);
        out.push({ name, family: name.startsWith('mlx:') ? 'mlx' : 'gguf', remote: false });
    }
    return out.sort((a, b) => a.name.localeCompare(b.name));
}
/** Installed model references: Ollama's local store plus any tags the active
 *  endpoint reports. Local weights are listed as `ollama:<tag>`; Ollama exposes
 *  no cloud catalog, so cloud candidates must be configured. */
export async function listInstalledModels() {
    return (await listInstalled()).map(m => m.name);
}
/** Simultaneous probes allowed while scanning.
 *
 *  Previously every installed model was probed with Promise.all, so a host with
 *  30 models got 30 simultaneous requests — enough to trip rate limits, and on a
 *  cloud endpoint every one of those is a billed call. Bounded, with results
 *  kept in input order so the displayed list is stable. */
const PROBE_CONCURRENCY = 4;
async function mapLimit(items, limit, fn) {
    const out = new Array(items.length);
    let next = 0;
    const workers = Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, async () => {
        while (next < items.length) {
            const i = next++;
            out[i] = await fn(items[i]);
        }
    });
    await Promise.all(workers);
    return out;
}
/** Probe every installed model and return those that can actually call tools. */
export async function scanForToolCapable(models, timeoutMs) {
    // Native models share one Swift process and one loaded model, so requests
    // queue behind each other. Probing four at once would start four timers and
    // let the last three expire while they wait, even though the runtime is
    // healthy. Serialize when any candidate is native; HTTP providers still get
    // the bounded parallel path.
    const native = models.some(model => {
        try {
            return resolveLocalModel(model) !== null;
        }
        catch {
            return true;
        }
    });
    const settled = await mapLimit(models, native ? 1 : PROBE_CONCURRENCY, m => probeModel(m, timeoutMs));
    return {
        capable: settled.filter(r => r.status === 'ok'),
        rejected: settled.filter(r => r.status !== 'ok'),
    };
}
