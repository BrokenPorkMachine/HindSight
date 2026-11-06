/**
 * Model selection.
 *
 * Single implementation of the fallback chain, shared by the REPL and the
 * diagnostic entrypoint. This previously existed twice with divergent behaviour
 * (only one of them persisted probe results, only one filtered known-bad
 * candidates) — a drift hazard on exactly the path that decides what the agent
 * is allowed to run on.
 *
 * Chain: configured candidates -> scan installed models -> hard failure with
 * per-model reasons. Never degrades silently to a model that cannot call tools.
 */
import { saveConfig } from '../config.js';
import { listInstalled, listInstalledModels, scanForToolCapable, selectModel } from './probe.js';
import { activeProvider, applyConfiguredProvider, builtinProvider, DEFAULT_HOST, DEFAULT_CLOUD_HOST, normalizeHost, resolveProvider, setActiveProvider } from './provider.js';
import { listLocalNativeModels, listLocalOllamaModels } from './local-models.js';
function defaultPick(capable) {
    const first = capable[0];
    if (capable.length === 1)
        return Promise.resolve(first.model);
    return Promise.resolve(first.model);
}
/** Discover every known Ollama surface without mixing same-named models on different hosts. */
async function discoverModel(cfg, deps) {
    const original = activeProvider();
    const builtin = builtinProvider();
    const surfaces = [original, builtin,
        resolveProvider({ name: 'local', host: DEFAULT_HOST }),
        ...cfg.providers.list.map(p => resolveProvider(p)),
    ];
    if (process.env['OLLAMA_API_KEY']) {
        surfaces.push({ ...builtin, name: 'ollama-cloud',
            host: normalizeHost(process.env['OLLAMA_CLOUD_HOST'] ?? DEFAULT_CLOUD_HOST) });
    }
    const seen = new Set();
    const providers = surfaces.filter(p => {
        const key = JSON.stringify([p.host, p.apiKey ?? '']);
        if (seen.has(key))
            return false;
        seen.add(key);
        return true;
    });
    const listings = await Promise.all(providers.map(async (provider) => ({
        provider, models: await listInstalled(provider),
    })));
    const choices = new Map();
    const results = [];
    // A machine with downloaded Ollama GGUF weights must work even if the
    // daemon is stopped. Probe native references independently of HTTP hosts.
    const local = listLocalOllamaModels();
    if (local.length) {
        const scan = await scanForToolCapable(local);
        for (const result of [...scan.capable, ...scan.rejected]) {
            const label = `${result.model} @ native (offline)`;
            results.push({ ...result, model: label });
            deps.report?.({ ...result, model: label });
            if (result.status === 'ok')
                choices.set(label, { model: result.model });
        }
    }
    // Loose native weights (Warhead's pull store, Hugging Face cache) are a
    // second tier: probing every multi-GB checkpoint is slow, so only reach for
    // them when the Ollama store produced no tool-capable model at all.
    if (!choices.size) {
        const native = listLocalNativeModels();
        if (native.length) {
            const scan = await scanForToolCapable(native);
            for (const result of [...scan.capable, ...scan.rejected]) {
                const label = `${result.model} @ native (offline)`;
                results.push({ ...result, model: label });
                deps.report?.({ ...result, model: label });
                if (result.status === 'ok')
                    choices.set(label, { model: result.model });
            }
        }
    }
    try {
        for (const { provider, models } of listings) {
            setActiveProvider(provider);
            // listInstalled also includes the same local manifest references: do not
            // probe them once for every configured network provider.
            const scan = await scanForToolCapable(models.map(m => m.name).filter(name => !name.startsWith('ollama:')));
            for (const result of [...scan.capable, ...scan.rejected]) {
                const label = `${result.model} @ ${provider.name} (${provider.host})`;
                const displayed = { ...result, model: label };
                results.push(displayed);
                deps.report?.(displayed);
                if (result.status === 'ok')
                    choices.set(label, { provider, model: result.model });
            }
        }
    }
    finally {
        applyConfiguredProvider(cfg);
    }
    const capable = results.filter(r => r.status === 'ok');
    if (!capable.length)
        throw new Error('no tool-capable models found in the local Ollama store or configured endpoints. Use mlx:<repo-or-path>, gguf:<path>, or install a compatible model.');
    const label = await (deps.pick ?? defaultPick)(capable);
    const selected = choices.get(label);
    if (!selected)
        throw new Error('selected model is not in the discovered model list');
    if (deps.persist ?? true) {
        // Native inference never needs a provider. Keep an existing provider
        // configuration intact while making the local choice the next candidate.
        if (!selected.provider) {
            cfg.model.candidates = [selected.model];
            cfg.model.lastUsed = selected.model;
            cfg.model.known[selected.model] = { status: 'ok', detail: 'tool call verified during offline discovery' };
            saveConfig(cfg);
            return { model: selected.model, results, via: capable.length === 1 ? 'only-option' : 'scan' };
        }
        // Capture the narrowed endpoint before entering a callback. TypeScript cannot
        // retain property narrowing across the find() closure.
        const provider = selected.provider;
        // Retain the endpoint credentials by reference where possible, never copy an env key inline.
        let entry = cfg.providers.list.find(p => {
            const resolved = resolveProvider(p);
            return resolved.host === provider.host && resolved.apiKey === provider.apiKey;
        });
        if (!entry) {
            let name = provider.name;
            while (cfg.providers.list.some(p => p.name === name))
                name += '-discovered';
            entry = { name, host: provider.host,
                ...(provider.keySource === 'env' ? { apiKeyEnv: 'OLLAMA_API_KEY' } : {}) };
            cfg.providers.list.push(entry);
        }
        cfg.providers.active = entry.name;
        cfg.model.candidates = [selected.model];
        cfg.model.known[selected.model] = { status: 'ok', detail: 'tool call verified during discovery' };
        saveConfig(cfg);
        applyConfiguredProvider(cfg);
    }
    else if (selected.provider) {
        setActiveProvider(selected.provider);
    }
    return { model: selected.model, results, via: capable.length === 1 ? 'only-option' : 'scan' };
}
export async function resolveModel(cfg, deps = {}) {
    if (cfg.model.candidates.length === 0 && !cfg.model.lastUsed && !process.env['WARHEAD_MODEL']?.trim())
        return discoverModel(cfg, deps);
    const report = deps.report ?? (() => { });
    const pick = deps.pick ?? defaultPick;
    const persist = deps.persist ?? true;
    // Skip candidates already recorded as non-functional, unless that would leave
    // nothing to try — a stale "unsupported" must not permanently exclude a model.
    const explicit = process.env['WARHEAD_MODEL']?.trim();
    // Build queue: explicit env > configured candidates > last used > server list
    let candidates = [];
    if (explicit) {
        candidates = [explicit];
    }
    else {
        candidates = [...(cfg.model.candidates ?? [])];
        if (cfg.model.lastUsed && !candidates.includes(cfg.model.lastUsed)) {
            candidates.push(cfg.model.lastUsed);
        }
        if (candidates.length === 0) {
            // Pull available tags from the server and default to the first
            const installed = await listInstalledModels();
            candidates = installed.length > 0 ? installed : [];
        }
    }
    const viable = candidates.filter(m => cfg.model.known[m]?.status !== 'unsupported');
    const queue = viable.length > 0 ? viable : candidates;
    const first = await selectModel(queue);
    first.results.forEach(report);
    if (first.chosen) {
        if (persist) {
            const last = first.results.at(-1);
            if (last)
                cfg.model.known[first.chosen] = { status: last.status, detail: last.detail };
            cfg.model.lastUsed = first.chosen;
            saveConfig(cfg);
        }
        return { model: first.chosen, results: first.results, via: 'candidate' };
    }
    // An explicit environment selection is authoritative. Scanning other models
    // here would make a typo or unsupported request silently choose a different
    // model than the operator configured.
    if (explicit) {
        throw new Error([`WARHEAD_MODEL (${explicit}) cannot call tools:`,
            ...first.results.map(r => `  ${r.model} — ${r.status}: ${r.detail}`)].join('\n'));
    }
    if (!cfg.model.scanInstalledOnFailure) {
        throw new Error([
            'no configured model can call tools, and scanning installed models is disabled.',
            ...first.results.map(r => `  ${r.model} — ${r.status}: ${r.detail}`),
        ].join('\n'));
    }
    const installed = await listInstalledModels();
    if (installed.length === 0) {
        if (first?.results?.length && first.results.every(r => r.status === 'unreachable')) {
            // allow proxy path
            return { model: candidates[0], results: first.results, via: 'candidate' };
        }
        if (first?.chosen)
            return { model: first.chosen, results: first.results, via: 'candidate' };
        throw new Error([
            'no models are installed, and none of the configured candidates resolved.',
            ...first.results.map(r => `  ${r.model} — ${r.status}: ${r.detail}`),
        ].join('\n'));
    }
    const { capable, rejected } = await scanForToolCapable(installed);
    rejected.forEach(report);
    if (persist)
        for (const r of rejected)
            cfg.model.known[r.model] = { status: r.status, detail: r.detail };
    if (capable.length === 0) {
        throw new Error([
            'no available model can call tools.',
            '',
            'rejected:',
            ...first.results.map(r => `  ${r.model} — ${r.status}: ${r.detail}`),
            ...rejected.map(r => `  ${r.model} — ${r.status}: ${r.detail}`),
        ].join('\n'));
    }
    const model = await pick(capable);
    if (persist) {
        cfg.model.lastUsed = model;
        for (const r of capable)
            cfg.model.known[r.model] = { status: 'ok', detail: r.detail };
        saveConfig(cfg);
    }
    return { model, results: [...first.results, ...capable, ...rejected], via: capable.length === 1 ? 'only-option' : 'scan' };
}
