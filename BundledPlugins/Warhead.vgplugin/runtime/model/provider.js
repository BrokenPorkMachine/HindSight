/**
 * Providers: named Ollama endpoints.
 *
 * A provider is a host plus credentials, not a pluggable backend. Every Ollama
 * endpoint — a local daemon, a box on the LAN, a cloud gateway — speaks the same
 * `/api/chat` protocol, so switching between them needs no new streaming or
 * tool-call code. That is the whole reason this is a small type rather than an
 * interface: a genuinely different vendor (OpenAI streams SSE `tool_calls`
 * deltas, Anthropic a different message shape again) does not fit behind this
 * and would need a real adapter layer.
 *
 * Resolution order, most specific first:
 *
 *   1. the provider named by `providers.active` in config
 *   2. $OLLAMA_HOST
 *   3. http://localhost:11434
 *
 * Config wins over the environment because naming a provider is a deliberate act
 * and an ambient env var is not — but with no active provider configured, the
 * env var still applies, which is what keeps `OLLAMA_HOST=<fake> npm test` and
 * ordinary shell overrides working untouched.
 */
import { AsyncLocalStorage } from 'node:async_hooks';
export const DEFAULT_HOST = process.env['WARHEAD_INFERENCE_UPSTREAM'] || process.env['OLLAMA_HOST'] || 'http://localhost:11434';
export const DEFAULT_CLOUD_HOST = 'https://ollama.com';
export class ProviderError extends Error {
}
/** Accept what a person would actually type.
 *
 *  `localhost:11434` has no scheme, and `fetch` rejects it outright with an
 *  opaque "Failed to parse URL". Ollama's own OLLAMA_HOST convention is a bare
 *  host:port, so requiring a scheme here would reject the form the tool's own
 *  documentation uses. */
export function normalizeHost(raw) {
    const trimmed = raw.trim();
    if (trimmed === '')
        throw new ProviderError('host is empty');
    // Detect *any* scheme, not just http/https. Testing for `^https?://` instead
    // meant `ftp://example.com` failed the test, got `http://` prepended, and
    // parsed as the host `ftp` — silently pointing at the wrong machine rather
    // than rejecting the input.
    const hasScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed);
    const withScheme = hasScheme ? trimmed : `http://${trimmed}`;
    let url;
    try {
        url = new URL(withScheme);
    }
    catch {
        throw new ProviderError(`not a usable host: ${raw}`);
    }
    if (url.protocol !== 'http:' && url.protocol !== 'https:') {
        throw new ProviderError(`host must be http or https, got ${url.protocol.replace(':', '')}`);
    }
    if (url.hostname === '')
        throw new ProviderError(`not a usable host: ${raw}`);
    if (url.username || url.password)
        throw new ProviderError('host must not contain credentials; configure an API key instead');
    if (url.search || url.hash)
        throw new ProviderError('host must not contain a query string or fragment');
    // Trailing slash removed so `${host}/api/chat` cannot become `//api/chat`.
    return `${url.origin}${url.pathname.replace(/\/+$/, '')}`;
}
export function resolveProvider(p, builtin = false) {
    const host = normalizeHost(p.host);
    // An env var is consulted at call time, not at load time, so exporting a key
    // after startup works and a short-lived credential never has to be written
    // to disk at all.
    const fromEnv = p.apiKeyEnv ? process.env[p.apiKeyEnv] : undefined;
    if (fromEnv) {
        return { name: p.name, host, apiKey: fromEnv, keySource: 'env', builtin };
    }
    if (p.apiKey) {
        return { name: p.name, host, apiKey: p.apiKey, keySource: 'inline', builtin };
    }
    return { name: p.name, host, keySource: 'none', builtin };
}
/** The fallback used when no provider is configured. */
export function builtinProvider() {
    const env = process.env['OLLAMA_HOST'];
    const host = env ? normalizeHost(env) : DEFAULT_HOST;
    // OLLAMA_API_KEY is the convention Ollama's own tooling uses, so honour it
    // here rather than requiring every cloud user to register a provider first.
    const key = process.env['OLLAMA_API_KEY'];
    return {
        name: 'default',
        host,
        apiKey: key,
        keySource: key ? 'env' : 'none',
        builtin: true,
    };
}
// ---------------------------------------------------------------- active state
let override = null;
const requestProvider = new AsyncLocalStorage();
/** Scope parallel requests to their own endpoint without switching the REPL's
 * selected provider or sending another model's credentials to the wrong host. */
export function withProvider(provider, run) {
    return requestProvider.run(provider, run);
}
/** The provider every request goes to. */
export function activeProvider() {
    return requestProvider.getStore() ?? override ?? builtinProvider();
}
/** Resolve a model request. Explicitly selected providers always win. For the
 * built-in provider, authenticated `-cloud` tags use Ollama Cloud directly;
 * this keeps `OLLAMA_API_KEY=... warhead-cli` useful without requiring a
 * provider entry, while preserving local Ollama for local models. */
export function providerForModel(model) {
    const active = activeProvider();
    if (requestProvider.getStore() || override || !active.builtin || !/-cloud(?::|$)/i.test(model) || !active.apiKey)
        return active;
    const host = process.env['OLLAMA_CLOUD_HOST']
        ? normalizeHost(process.env['OLLAMA_CLOUD_HOST'])
        : DEFAULT_CLOUD_HOST;
    return { ...active, name: 'ollama-cloud', host };
}
/** Point the process at a provider. `null` restores the built-in fallback. */
export function setActiveProvider(p) {
    override = p;
}
/** Adopt `providers.active` from config. Called once at startup and again after
 *  `/provider use`. A dangling name falls back rather than throwing, so a config
 *  edited by hand cannot leave the tool unable to start. */
export function applyConfiguredProvider(cfg) {
    const name = cfg.providers.active;
    if (!name) {
        setActiveProvider(null);
        return builtinProvider();
    }
    const found = findProvider(cfg, name);
    if (!found) {
        setActiveProvider(null);
        return builtinProvider();
    }
    const resolved = resolveProvider(found);
    setActiveProvider(resolved);
    return resolved;
}
// ------------------------------------------------------------------- lookups
export function findProvider(cfg, name) {
    const want = name.trim().toLowerCase();
    return cfg.providers.list.find(p => p.name.trim().toLowerCase() === want);
}
export function addProvider(cfg, p) {
    const i = cfg.providers.list.findIndex(x => x.name.trim().toLowerCase() === p.name.trim().toLowerCase());
    if (i === -1) {
        cfg.providers.list.push(p);
        return { added: true, replaced: false };
    }
    cfg.providers.list[i] = p;
    return { added: false, replaced: true };
}
export function removeProvider(cfg, name) {
    const want = name.trim().toLowerCase();
    const i = cfg.providers.list.findIndex(p => p.name.trim().toLowerCase() === want);
    if (i === -1)
        return false;
    cfg.providers.list.splice(i, 1);
    // Clearing the pointer matters: leaving `active` naming a deleted provider is
    // how a config ends up silently falling back with nothing on screen saying so.
    if (cfg.providers.active?.trim().toLowerCase() === want)
        cfg.providers.active = null;
    return true;
}
/** Auth headers for a provider. Ollama cloud takes a bearer token; a local
 *  daemon takes nothing, and sending an empty header would be worse than none. */
export function authHeaders(p = activeProvider()) {
    return p.apiKey ? { authorization: `Bearer ${p.apiKey}` } : {};
}
/** One-line description for display. Never includes the key. */
export function describeProvider(p) {
    const key = p.keySource === 'none'
        ? 'no key'
        : p.keySource === 'env'
            ? 'key from env'
            : 'key inline (plaintext)';
    return `${p.name}  ${p.host}  [${key}${p.builtin ? ', built-in' : ''}]`;
}
