/** Serverless model acquisition. All downloads are streamed, bounded, and
 * published only after transfer completion. Nothing launches Ollama.
 * HF requires compatible MLX or GGUF weights; arbitrary HF checkpoints are NOT executable. */
import { createHash, randomUUID } from 'node:crypto';
import { createWriteStream, existsSync, statSync } from 'node:fs';
import { open, link, unlink, mkdir } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { ollamaModelsRoot } from './local-models.js';
const BASE = join(homedir(), '.warhead', 'models');
const MAX_FILE = 100 * 1024 ** 3;
const MAX_SNAPSHOT = 150 * 1024 ** 3;
const DOWNLOAD_TIMEOUT_MS = 30 * 60_000;
const METADATA_TIMEOUT_MS = 30_000;
const boundedSignal = (signal, timeout = METADATA_TIMEOUT_MS) => AbortSignal.any([...(signal ? [signal] : []), AbortSignal.timeout(timeout)]);
const checkedFetch = async (url, headers, signal, timeout = METADATA_TIMEOUT_MS) => {
    const response = await fetch(url, { headers, signal: boundedSignal(signal, timeout) });
    const endpoint = new URL(response.url);
    if (endpoint.protocol !== 'https:' || !(['huggingface.co', 'cdn-lfs.huggingface.co', 'cas-bridge.xethub.hf.co', 'cas-server.xethub.hf.co', 'registry.ollama.ai'].includes(endpoint.hostname) || endpoint.hostname.endsWith('.hf.co') || endpoint.hostname.endsWith('.huggingface.co') || endpoint.hostname.endsWith('.ollama.ai'))) {
        await response.body?.cancel();
        throw new Error('model download redirected to an untrusted host');
    }
    return response;
};
const hfHeaders = () => process.env['HF_TOKEN'] ? { authorization: `Bearer ${process.env['HF_TOKEN']}` } : {};
const validSegment = (v) => /^[A-Za-z0-9][A-Za-z0-9_.-]*$/.test(v) && v !== '..';
const safeRepo = (id) => {
    const parts = id.split('/');
    if (parts.length !== 2 || parts.some(p => !validSegment(p)))
        throw new Error('expected a Hugging Face repository in owner/name format');
    return parts;
};
const safeRelative = (p) => {
    if (!p || p.startsWith('/') || p.includes('\\') || p.split('/').some(v => !v || v === '.' || v === '..'))
        throw new Error('unsafe model filename');
    return p;
};
async function download(url, dest, headers, signal, expectedDigest, maxBytes = MAX_FILE) {
    if (existsSync(dest)) {
        if (!expectedDigest)
            return statSync(dest).size;
        const { createReadStream } = await import('node:fs');
        const existing = createHash('sha256');
        for await (const chunk of createReadStream(dest))
            existing.update(chunk);
        if (existing.digest('hex') === expectedDigest)
            return statSync(dest).size;
        throw new Error(`existing file has incorrect digest; refusing overwrite: ${dest}`);
    }
    await mkdir(dirname(dest), { recursive: true });
    const tmp = `${dest}.partial-${randomUUID()}`;
    const response = await checkedFetch(url, headers, signal, DOWNLOAD_TIMEOUT_MS);
    if (!response.ok || !response.body)
        throw new Error(`download HTTP ${response.status}: ${url}`);
    const declared = Number(response.headers.get('content-length'));
    if (declared > maxBytes)
        throw new Error('model file exceeds configured download size limit');
    let bytes = 0;
    const hash = createHash('sha256');
    const meter = new Transform({ transform(chunk, _encoding, done) {
            bytes += chunk.length;
            if (bytes > maxBytes)
                return done(new Error('model file exceeded download limit'));
            hash.update(chunk);
            done(null, chunk);
        } });
    try {
        await pipeline(Readable.fromWeb(response.body), meter, createWriteStream(tmp, { flags: 'wx', mode: 0o600 }), { signal: boundedSignal(signal, DOWNLOAD_TIMEOUT_MS) });
        if (expectedDigest && hash.digest('hex') !== expectedDigest)
            throw new Error('model blob SHA-256 verification failed');
        await link(tmp, dest); // never overwrite an existing model artifact
        return bytes;
    }
    finally {
        await unlink(tmp).catch(() => { });
    }
}
export async function pullHuggingFace(repoId, options = {}) {
    safeRepo(repoId);
    if (options.file && options.engine === 'mlx')
        throw new Error('--file and --mlx cannot be combined');
    const api = await checkedFetch(`https://huggingface.co/api/models/${repoId}?blobs=true`, hfHeaders(), options.signal);
    if (!api.ok)
        throw new Error(`Hugging Face model metadata HTTP ${api.status}; check HF_TOKEN and repository access`);
    const metadata = await api.json();
    const siblings = metadata.siblings ?? [];
    const ggufs = siblings.filter(f => f.rfilename.toLowerCase().endsWith('.gguf'));
    const engine = options.engine ?? (options.file || ggufs.length ? 'gguf' : 'mlx');
    let targets;
    if (engine === 'gguf') {
        if (!options.file && ggufs.length !== 1)
            throw new Error(`repository has ${ggufs.length} GGUF files; specify --file <repo-relative .gguf filename>`);
        const file = safeRelative(options.file ?? ggufs[0].rfilename);
        if (!file.toLowerCase().endsWith('.gguf'))
            throw new Error('native GGUF requires a .gguf file');
        targets = siblings.filter(f => f.rfilename === file);
        if (!targets.length)
            throw new Error(`GGUF file not found in repository: ${file}`);
    }
    else {
        targets = siblings.filter(f => /(?:^|\/)(?:config\.json|generation_config\.json|tokenizer[^/]*|special_tokens_map\.json|added_tokens\.json|vocab[^/]*|merges\.txt|preprocessor_config\.json|(?:model\.)?safetensors\.index\.json|model[^/]*\.safetensors|[^/]+\.safetensors|[^/]+\.tiktoken)$/i.test(f.rfilename));
        if (!targets.some(f => f.rfilename.endsWith('.safetensors')))
            throw new Error('repository has no MLX safetensors; use a compatible MLX-converted checkpoint');
        if (!targets.some(f => f.rfilename === 'config.json'))
            throw new Error('MLX checkpoint requires config.json');
    }
    const root = resolve(BASE, 'hf', ...safeRepo(repoId));
    // Validate every path before downloading anything, rather than discovering
    // malicious repository metadata halfway through a partially written snapshot.
    for (const file of targets)
        safeRelative(file.rfilename);
    let total = 0;
    for (const file of targets) {
        const relative = safeRelative(file.rfilename);
        const size = file.lfs?.size ?? file.size ?? 0;
        if (size > MAX_FILE || total + size > MAX_SNAPSHOT)
            throw new Error('model snapshot exceeds download size limit');
        options.report?.(`downloading ${repoId}/${relative}`);
        const dest = join(root, relative);
        const url = `https://huggingface.co/${repoId}/resolve/main/${relative.split('/').map(encodeURIComponent).join('/')}?download=true`;
        const bytes = await download(url, dest, hfHeaders(), options.signal, file.lfs?.oid?.replace(/^sha256:/, ''));
        total += bytes;
        if (total > MAX_SNAPSHOT)
            throw new Error('model snapshot exceeds 150 GiB');
    }
    const path = engine === 'gguf' ? join(root, safeRelative(targets[0].rfilename)) : root;
    return { ref: `${engine}:${path}`, files: targets.length, downloadedBytes: total };
}
function parseOllama(tag) {
    const colon = tag.lastIndexOf(':');
    const slash = tag.lastIndexOf('/');
    const name = colon > slash ? tag.slice(0, colon) : tag;
    const version = colon > slash ? tag.slice(colon + 1) : 'latest';
    const path = name.includes('/') ? name : `library/${name}`;
    if (!path.split('/').every(validSegment) || !validSegment(version))
        throw new Error('invalid Ollama model name/tag');
    return { path, version, tag: `${name}:${version}` };
}
export async function pullOllamaNative(tag, options = {}) {
    const parsed = parseOllama(tag);
    const base = 'https://registry.ollama.ai/v2';
    const manifestUrl = `${base}/${parsed.path}/manifests/${parsed.version}`;
    let auth = '';
    let response = await checkedFetch(manifestUrl, { accept: 'application/vnd.docker.distribution.manifest.v2+json' }, options.signal);
    if (response.status === 401) {
        const challenge = response.headers.get('www-authenticate') ?? '';
        const realm = /realm="([^"]+)"/.exec(challenge)?.[1];
        if (!realm)
            throw new Error('Ollama registry authentication challenge missing realm');
        const authUrl = new URL(realm);
        if (authUrl.protocol !== 'https:' || authUrl.hostname !== 'ollama.ai' && !authUrl.hostname.endsWith('.ollama.ai'))
            throw new Error('unexpected registry authentication host');
        const service = /service="([^"]+)"/.exec(challenge)?.[1];
        const scope = /scope="([^"]+)"/.exec(challenge)?.[1];
        if (service)
            authUrl.searchParams.set('service', service);
        if (scope)
            authUrl.searchParams.set('scope', scope);
        const tokenRes = await checkedFetch(authUrl.toString(), {}, options.signal);
        if (!tokenRes.ok)
            throw new Error(`registry token HTTP ${tokenRes.status}`);
        const token = await tokenRes.json();
        auth = token.token ?? token.access_token ?? '';
        if (!auth)
            throw new Error('registry did not return a token');
        response = await checkedFetch(manifestUrl, { authorization: `Bearer ${auth}` }, options.signal);
    }
    if (!response.ok)
        throw new Error(`Ollama manifest HTTP ${response.status}`);
    const raw = await response.text();
    if (raw.length > 4 * 1024 * 1024)
        throw new Error('Ollama manifest too large');
    const manifest = JSON.parse(raw);
    if (manifest.schemaVersion !== 2 || !Array.isArray(manifest.layers))
        throw new Error('unsupported Ollama manifest');
    const modelLayer = manifest.layers.find(l => l.mediaType === 'application/vnd.ollama.image.model');
    if (!modelLayer)
        throw new Error('Ollama tag is not a locally executable model');
    const layers = [...manifest.layers, ...(manifest.config ? [manifest.config] : [])];
    const root = ollamaModelsRoot();
    let bytes = 0;
    for (const layer of layers) {
        if (!Number.isSafeInteger(layer.size) || (layer.size ?? 0) < 0 || (layer.size ?? 0) > MAX_FILE || bytes + (layer.size ?? 0) > MAX_SNAPSHOT)
            throw new Error('Ollama layer size exceeds configured download limits');
        if (!/^sha256:[a-f0-9]{64}$/.test(layer.digest))
            throw new Error('invalid Ollama layer digest');
        const hash = layer.digest.slice(7);
        options.report?.(`pulling Ollama blob ${hash.slice(0, 12)}`);
        bytes += await download(`${base}/${parsed.path}/blobs/${layer.digest}`, join(root, 'blobs', `sha256-${hash}`), auth ? { authorization: `Bearer ${auth}` } : {}, options.signal, hash);
        if (bytes > MAX_SNAPSHOT)
            throw new Error('model pull exceeds 150 GiB');
    }
    const manifestPath = join(root, 'manifests', 'registry.ollama.ai', parsed.path, parsed.version);
    await mkdir(dirname(manifestPath), { recursive: true });
    if (!existsSync(manifestPath)) {
        const fd = await open(manifestPath, 'wx', 0o600);
        try {
            await fd.writeFile(raw);
        }
        finally {
            await fd.close();
        }
    }
    return { ref: `ollama:${parsed.tag}`, downloadedBytes: bytes, files: layers.length };
}
