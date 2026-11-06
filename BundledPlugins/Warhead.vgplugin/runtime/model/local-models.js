/** Resolve model references that Warhead can load without an Ollama daemon.
 * Ollama's local manifest is only used as an index: inference consumes the GGUF
 * blob directly through the native llama.cpp runtime. */
import { execFileSync } from 'node:child_process';
import { closeSync, existsSync, openSync, readFileSync, readdirSync, readSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, join, resolve, sep } from 'node:path';
export class LocalModelError extends Error {
    constructor(message) { super(message); this.name = 'LocalModelError'; }
}
const MODEL_LAYER = 'application/vnd.ollama.image.model';
const SYSTEM_LAYER = 'application/vnd.ollama.image.system';
const TEMPLATE_LAYER = 'application/vnd.ollama.image.template';
const PARAMS_LAYER = 'application/vnd.ollama.image.params';
const DIGEST = /^sha256:[0-9a-f]{64}$/;
/** Manifest layouts both appear in the wild: `manifests/<registry-host>/…` is
 *  the classic layout, while current builds also write `manifests-v2/…` with
 *  the newer `ollama.com` registry name. Reading only the first one hides
 *  models that the running daemon happily lists. */
const MANIFEST_DIRS = ['manifests', 'manifests-v2'];
function defaultOllamaRoots() {
    const home = homedir();
    const xdgData = process.env['XDG_DATA_HOME'] || join(home, '.local', 'share');
    return [
        join(home, '.ollama', 'models'),
        join(xdgData, 'ollama', 'models'),
        // macOS app bundle default when the app is pointed at its own store.
        join(home, 'Library', 'Application Support', 'Ollama', 'models'),
    ];
}
let daemonRoot;
/** Where the *running* daemon keeps its store, when it can be determined.
 *
 *  Ollama accepts `OLLAMA_MODELS` but exposes no API to read it back, so a
 *  store outside the default location is invisible to an offline reader even
 *  though `/api/tags` lists its models. On systems where the process table is
 *  readable for the same user, take the value from the daemon's environment.
 *  Best effort and cached: any failure just means "use the documented
 *  defaults". Never throws. */
function daemonModelsRoot() {
    if (daemonRoot !== undefined)
        return daemonRoot;
    daemonRoot = null;
    if (process.platform !== 'darwin' && process.platform !== 'linux')
        return daemonRoot;
    try {
        const listing = execFileSync('ps', ['-Ao', 'pid=,command='], {
            encoding: 'utf8', timeout: 2_000, stdio: ['ignore', 'pipe', 'ignore'],
        });
        const pids = listing.split('\n')
            .filter(line => /\bollama\b/i.test(line) && /\bserve\b/i.test(line))
            .map(line => line.trim().split(/\s+/, 1)[0])
            .filter(pid => /^\d+$/.test(pid))
            .slice(0, 4);
        for (const pid of pids) {
            let env;
            try {
                env = execFileSync('ps', ['eww', '-p', pid], {
                    encoding: 'utf8', timeout: 2_000, stdio: ['ignore', 'pipe', 'ignore'],
                });
            }
            catch {
                continue;
            }
            // Environment values may contain spaces (a path under "Application Support"),
            // so stop at the next `NAME=` token instead of at the first space.
            const match = /(?:^|\s)OLLAMA_MODELS=(.*?)(?=\s+[A-Za-z_][A-Za-z0-9_]*=|$)/s.exec(env);
            const value = match?.[1]?.trim();
            if (!value)
                continue;
            const candidate = resolve(value);
            if (existsSync(join(candidate, 'manifests')) || existsSync(join(candidate, 'manifests-v2')) || existsSync(join(candidate, 'blobs'))) {
                daemonRoot = candidate;
                break;
            }
        }
    }
    catch { /* process table unavailable; documented defaults still apply */ }
    return daemonRoot;
}
/** Every Ollama model store this host may have weights in, most authoritative
 *  first. `OLLAMA_MODELS` may list several roots separated by the platform
 *  path delimiter, which keeps a per-project store discoverable. */
export function ollamaModelRoots() {
    const roots = [];
    const add = (value) => {
        for (const part of (value ?? '').split(delimiter)) {
            const trimmed = part.trim();
            if (!trimmed)
                continue;
            const root = resolve(trimmed);
            if (!roots.includes(root))
                roots.push(root);
        }
    };
    // Ollama treats OLLAMA_MODELS as the whole store, not an extra location, so an
    // explicit value must not be mixed with the daemon's store or the defaults:
    // that would resurrect deleted models from a stale directory.
    const explicit = process.env['OLLAMA_MODELS'];
    if (explicit?.trim()) {
        add(explicit);
        if (roots.length)
            return roots;
    }
    add(daemonModelsRoot());
    for (const root of defaultOllamaRoots())
        add(root);
    return roots;
}
/** The store Warhead writes to and reads first. */
export function ollamaModelsRoot() {
    return ollamaModelRoots()[0];
}
function readLayerText(root, digest) {
    if (typeof digest !== 'string' || !DIGEST.test(digest))
        return undefined;
    try {
        const text = readFileSync(join(root, 'blobs', digest.replace(':', '-')), 'utf8').trim();
        return text || undefined;
    }
    catch {
        return undefined;
    }
}
/** Modelfile parameters are stored as a JSON object, e.g. `{"stop":["</s>"]}`. */
function parseParams(text) {
    if (!text)
        return {};
    try {
        const parsed = JSON.parse(text);
        const stops = Array.isArray(parsed.stop)
            ? parsed.stop.filter((s) => typeof s === 'string' && s.length > 0)
            : [];
        return stops.length ? { stops: [...new Set(stops)] } : {};
    }
    catch {
        return {};
    }
}
function parseManifest(manifestPath, root) {
    let manifest;
    try {
        manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
    }
    catch (err) {
        throw new LocalModelError(`cannot read Ollama model manifest: ${err instanceof Error ? err.message : String(err)}`);
    }
    const layers = Array.isArray(manifest['layers']) ? manifest['layers'] : [];
    const model = layers.find(layer => layer['mediaType'] === MODEL_LAYER);
    const digest = model?.['digest'];
    if (typeof digest !== 'string' || !DIGEST.test(digest)) {
        throw new LocalModelError('Ollama manifest does not contain a valid model layer digest');
    }
    const modelBlob = join(root, 'blobs', digest.replace(':', '-'));
    if (!existsSync(modelBlob))
        throw new LocalModelError(`Ollama model blob is missing: ${modelBlob}`);
    // Ollama stores a Modelfile's SYSTEM/TEMPLATE/PARAMETERS text as their own
    // layers. Older layouts put the system prompt in the config blob instead, so
    // keep that as a fallback.
    let system;
    const systemLayer = layers.find(layer => layer['mediaType'] === SYSTEM_LAYER);
    const systemText = readLayerText(root, systemLayer?.['digest']);
    if (systemText)
        system = systemText;
    if (!system) {
        const config = manifest['config'];
        const configDigest = config?.['digest'];
        if (typeof configDigest === 'string' && DIGEST.test(configDigest)) {
            try {
                const parsed = JSON.parse(readFileSync(join(root, 'blobs', configDigest.replace(':', '-')), 'utf8'));
                if (typeof parsed['system'] === 'string' && parsed['system'].trim())
                    system = parsed['system'].trim();
            }
            catch { /* Modelfile metadata is optional; GGUF still contains its template. */ }
        }
    }
    // The manifest template is the format Ollama itself uses for this tag. A GGUF
    // pulled through Ollama frequently embeds a different (sometimes unrelated)
    // template, so the daemon and a native load would format prompts differently.
    const templateLayer = layers.find(layer => layer['mediaType'] === TEMPLATE_LAYER);
    const templateText = readLayerText(root, templateLayer?.['digest']);
    const template = templateText && templateText.includes('{{') ? templateText : undefined;
    const paramsLayer = layers.find(layer => layer['mediaType'] === PARAMS_LAYER);
    const { stops } = parseParams(readLayerText(root, paramsLayer?.['digest']));
    return { modelBlob, ...(system ? { system } : {}), ...(template ? { template } : {}), ...(stops ? { stops } : {}) };
}
/** Split `name[:tag]` into an Ollama repository path and tag. */
function splitOllamaRef(ref) {
    const slash = ref.lastIndexOf('/');
    const colon = ref.lastIndexOf(':');
    const tag = colon > slash ? ref.slice(colon + 1) : 'latest';
    const name = colon > slash ? ref.slice(0, colon) : ref;
    if (!name || name.split('/').some(part => !part || part === '.' || part === '..') || !/^[\w.-]+$/.test(tag) || !/^[\w./-]+$/.test(name)) {
        throw new LocalModelError(`invalid Ollama model reference: ${ref}`);
    }
    return { repository: name.includes('/') ? name : `library/${name}`, tag };
}
/** Registry host directory names used by Ollama on disk. Unknown hosts are
 *  still scanned because a mirrored registry writes its own host name. */
const REGISTRY_DIRS = ['registry.ollama.ai', 'ollama.com'];
function candidateManifestPaths(root, repository, tag, registry) {
    const hosts = registry ? [registry, ...REGISTRY_DIRS] : [...REGISTRY_DIRS];
    const paths = [];
    for (const dir of MANIFEST_DIRS) {
        for (const host of new Set(hosts))
            paths.push(join(root, dir, host, repository, tag));
    }
    return paths;
}
/** Resolve an `ollama:` tag against every known store, preferring the first. */
function findOllamaManifest(name) {
    const { repository, tag } = splitOllamaRef(name);
    for (const root of ollamaModelRoots()) {
        for (const path of candidateManifestPaths(root, repository, tag)) {
            if (existsSync(path))
                return { path, root };
        }
    }
    return null;
}
function walkManifests(root, dir, found) {
    const base = join(root, dir);
    if (!existsSync(base))
        return;
    const walk = (current, segments) => {
        let entries;
        try {
            entries = readdirSync(current, { withFileTypes: true });
        }
        catch {
            return;
        }
        for (const entry of entries) {
            const next = [...segments, entry.name];
            const path = join(current, entry.name);
            if (entry.isDirectory()) {
                walk(path, next);
                continue;
            }
            // `<registry>/<repository…>/<tag>`; anything shallower is not a manifest.
            if (!entry.isFile() || next.length < 3)
                continue;
            const tag = next.at(-1);
            const repository = next.slice(1, -1).join('/');
            if (!repository || !/^[\w.-]+$/.test(tag))
                continue;
            const name = repository.startsWith('library/') ? repository.slice('library/'.length) : repository;
            if (!name)
                continue;
            try {
                const loaded = parseManifest(path, root);
                // Embedding-only tags load without error but cannot chat; listing them
                // sends a first-run probe straight into a failure.
                const architecture = ggufArchitecture(loaded.modelBlob);
                if (architecture && NON_CHAT_GGUF_ARCH.test(architecture))
                    continue;
            }
            catch {
                continue;
            }
            const ref = `ollama:${name}:${tag}`;
            if (!found.includes(ref))
                found.push(ref);
        }
    };
    walk(base, []);
}
/** Supported references:
 *  - `mlx:<repo-id>` for a Hugging Face MLX checkpoint
 *  - `mlx:/absolute/path` for local MLX weights
 *  - `gguf:/absolute/model.gguf` for a GGUF file
 *  - `ollama:<name[:tag]>` for a blob already in an Ollama local store
 */
export function resolveLocalModel(ref) {
    if (ref.startsWith('mlx:')) {
        const id = ref.slice(4).trim();
        if (!id)
            throw new LocalModelError('MLX model reference is empty');
        if (id.startsWith('/')) {
            const path = resolve(id);
            if (!existsSync(path) || !statSync(path).isDirectory())
                throw new LocalModelError(`MLX model directory not found: ${path}`);
            return { engine: 'mlx', id: path, path };
        }
        if (id.split('/').some(part => !part || part === '.' || part === '..'))
            throw new LocalModelError(`invalid Hugging Face model id: ${id}`);
        return { engine: 'mlx', id };
    }
    if (ref.startsWith('gguf:')) {
        const path = resolve(ref.slice(5).trim());
        if (!path.toLowerCase().endsWith('.gguf') || !existsSync(path) || !statSync(path).isFile()) {
            throw new LocalModelError(`GGUF model file not found: ${path}`);
        }
        return { engine: 'gguf', id: path, path };
    }
    if (ref.startsWith('ollama:')) {
        const name = ref.slice(7).trim();
        if (!name)
            throw new LocalModelError('Ollama model reference is empty');
        const manifest = findOllamaManifest(name);
        if (!manifest)
            throw new LocalModelError(`Ollama model is not installed locally: ${name}`);
        const loaded = parseManifest(manifest.path, manifest.root);
        return {
            engine: 'gguf',
            id: `ollama:${name}`,
            path: loaded.modelBlob,
            ...(loaded.system ? { systemPrompt: loaded.system } : {}),
            ...(loaded.template ? { template: loaded.template } : {}),
            ...(loaded.stops ? { stops: loaded.stops } : {}),
        };
    }
    return null;
}
/** Enumerate GGUF model references without contacting an Ollama process.
 *
 *  Ollama stores a manifest at `<namespace>/<name>/<tag>`, one path segment per
 *  field. Rebuilding the reference from that path must turn the final segment
 *  back into a `:tag` — an earlier `ollama:name/tag` form parsed as a different
 *  repository and could not be loaded. Manifests without a readable model blob
 *  (cloud-only tags resolve to a manifest, not weights) are skipped: listing
 *  them would advertise a model Warhead cannot run offline. */
export function listLocalOllamaModels() {
    const found = [];
    for (const root of ollamaModelRoots()) {
        for (const dir of MANIFEST_DIRS)
            walkManifests(root, dir, found);
    }
    return found.sort();
}
/** Non-chat checkpoints that live in the same caches as language models. They
 *  would load-then-fail a tool probe, so they are not advertised. */
const NON_TEXT_MODEL_TYPES = new Set([
    'kokoro', 'whisper', 'wav2vec2', 'wavlm', 'hubert', 'seamless_m4t', 'speech_to_text',
    'vits', 'bark', 'speecht5', 'fastspeech2', 'tacotron2', 'musicgen', 'encodec',
    'clip', 'siglip', 'siglip_vision_model', 'vit', 'dinov2', 'sam', 'sam2', 'blip', 'blip2',
    'owlvit', 'grounding_dino', 'detr', 'rt_detr', 'swin', 'convnext',
]);
function isTextCheckpoint(directory) {
    const configPath = join(directory, 'config.json');
    if (!existsSync(configPath))
        return false;
    try {
        if (statSync(configPath).size > 1024 * 1024)
            return false;
        const config = JSON.parse(readFileSync(configPath, 'utf8'));
        const type = typeof config['model_type'] === 'string' ? config['model_type'].toLowerCase() : '';
        if (type && NON_TEXT_MODEL_TYPES.has(type))
            return false;
        const architectures = Array.isArray(config['architectures'])
            ? config['architectures'].filter((a) => typeof a === 'string').map(a => a.toLowerCase())
            : [];
        if (architectures.some(name => NON_TEXT_MODEL_TYPES.has(name.replace(/for.*$/, ''))))
            return false;
        // A speech/vocoder checkpoint (Kokoro is one) carries neither field at all,
        // so absence of both is the reliable signal -- assuming a config is enough
        // would advertise a TTS model that can only fail to load.
        return Boolean(type || architectures.length);
    }
    catch {
        return false;
    }
}
/** A Hugging Face cache stores each weight file as a symlink into `blobs/`, so
 *  `Dirent.isFile()` would miss every snapshot. Follow the link instead. */
function fileSize(path) {
    try {
        const info = statSync(path);
        return info.isFile() ? info.size : null;
    }
    catch {
        return null;
    }
}
/** Architectures that are not chat models. Ollama keeps no capability flag in
 *  its manifest, so an embedding-only tag (`nomic-embed-text`) looks exactly
 *  like a chat tag until the GGUF is loaded and fails. The architecture name in
 *  the GGUF header is the cheapest offline signal. */
const NON_CHAT_GGUF_ARCH = /bert|embed|rerank|clip|t5|whisper|wav2vec|vit\b|dino/i;
const GGUF_VALUE_SIZES = {
    0: 1, 1: 1, 2: 2, 3: 2, 4: 4, 5: 4, 6: 4, 7: 1, 10: 8, 11: 8, 12: 8,
};
/** Read `general.architecture` from a GGUF header without loading tensors.
 *  Cursor-bounded on purpose: a malformed or truncated file must return null
 *  rather than read past the buffer. */
function ggufArchitecture(path) {
    let buffer;
    try {
        const info = statSync(path);
        if (!info.isFile() || info.size < 24)
            return null;
        const handle = openSync(path, 'r');
        try {
            // Metadata precedes the tensor data; 256 KiB covers very long tokenizer lists.
            const length = Math.min(info.size, 256 * 1024);
            buffer = Buffer.allocUnsafe(length);
            const read = readSync(handle, buffer, 0, length, 0);
            buffer = buffer.subarray(0, read);
        }
        finally {
            closeSync(handle);
        }
    }
    catch {
        return null;
    }
    if (buffer.length < 24 || buffer.toString('latin1', 0, 4) !== 'GGUF')
        return null;
    let offset = 4;
    const need = (count) => offset + count <= buffer.length;
    const u32 = () => { if (!need(4))
        throw 0; const v = buffer.readUInt32LE(offset); offset += 4; return v; };
    const u64 = () => { if (!need(8))
        throw 0; const v = buffer.readBigUInt64LE(offset); offset += 8; return Number(v); };
    const str = () => {
        const length = u64();
        if (length > buffer.length || !need(length))
            throw 0;
        const value = buffer.toString('utf8', offset, offset + length);
        offset += length;
        return value;
    };
    const skip = (type, depth = 0) => {
        const size = GGUF_VALUE_SIZES[type];
        if (size !== undefined) {
            if (!need(size))
                throw 0;
            offset += size;
            return;
        }
        if (type === 8) {
            str();
            return;
        }
        if (type === 9) {
            if (depth > 0)
                throw 0;
            const element = u32();
            const count = u64();
            const elementSize = GGUF_VALUE_SIZES[element];
            if (elementSize !== undefined) {
                if (!need(elementSize * count))
                    throw 0;
                offset += elementSize * count;
                return;
            }
            for (let index = 0; index < count; index += 1)
                skip(element, depth + 1);
            return;
        }
        throw 0;
    };
    try {
        u32(); // version
        u64(); // tensor count
        const pairs = u64();
        // `general.architecture` is one of the first entries; stop early on oddities.
        for (let index = 0; index < Math.min(pairs, 256); index += 1) {
            const key = str();
            const type = u32();
            if (key === 'general.architecture' && type === 8)
                return str();
            skip(type);
        }
    }
    catch {
        return null;
    }
    return null;
}
function hasWeights(directory) {
    try {
        return readdirSync(directory, { withFileTypes: true })
            .some(entry => entry.name.endsWith('.safetensors') && fileSize(join(directory, entry.name)) !== null);
    }
    catch {
        return false;
    }
}
function listGgufFiles(directory) {
    try {
        const entries = readdirSync(directory, { withFileTypes: true });
        const out = [];
        for (const entry of entries) {
            if (!entry.name.toLowerCase().endsWith('.gguf'))
                continue;
            const path = join(directory, entry.name);
            const size = fileSize(path);
            // A few-KB `.gguf` is metadata or a stub, not runnable weights.
            if (size === null || size < 1024 * 1024)
                continue;
            out.push(path);
        }
        return out.sort();
    }
    catch {
        return [];
    }
}
/** Checkpoint directories that a single reference can point at, newest layout
 *  first. Warhead's own pull store is flat; Hugging Face keeps one snapshot
 *  directory per revision. */
function checkpointDirectories(root) {
    const out = [];
    const push = (directory) => {
        if (!out.includes(directory) && existsSync(directory))
            out.push(directory);
    };
    push(root);
    try {
        const snapshots = join(root, 'snapshots');
        if (!existsSync(snapshots))
            return out;
        const revisions = readdirSync(snapshots, { withFileTypes: true }).filter(entry => entry.isDirectory());
        // Prefer the most recently written snapshot, which is what the hub checks out.
        revisions
            .sort((a, b) => {
            try {
                return statSync(join(snapshots, b.name)).mtimeMs - statSync(join(snapshots, a.name)).mtimeMs;
            }
            catch {
                return 0;
            }
        })
            .slice(0, 1)
            .forEach(entry => push(join(snapshots, entry.name)));
    }
    catch { /* unreadable cache: the flat directory still counts */ }
    return out;
}
function warheadModelsRoot() {
    return process.env['WARHEAD_MODELS']?.trim() || join(homedir(), '.warhead', 'models');
}
function huggingFaceCacheRoots() {
    const roots = [];
    const add = (value) => {
        const trimmed = value?.trim();
        if (!trimmed)
            return;
        const root = resolve(trimmed);
        if (!roots.includes(root))
            roots.push(root);
    };
    // An explicit cache variable overrides the default location, the same way
    // huggingface_hub treats them. Falling back as well would mix a test or a
    // project-local cache with whatever the user already downloaded.
    const explicit = [process.env['HF_HUB_CACHE'], process.env['HUGGINGFACE_HUB_CACHE'], process.env['TRANSFORMERS_CACHE']];
    for (const value of explicit)
        add(value);
    if (roots.length)
        return roots;
    add(join(homedir(), '.cache', 'huggingface', 'hub'));
    return roots;
}
/** Native references for weights already on this disk: Warhead's own pull
 *  store and any Hugging Face cache. Both are invisible to Ollama, and asking
 *  a person to recover the absolute path by hand is the difference between a
 *  model that loads and a model that is never used. */
export function listLocalNativeModels() {
    const found = [];
    const add = (ref) => { if (!found.includes(ref))
        found.push(ref); };
    const warheadHf = join(warheadModelsRoot(), 'hf');
    try {
        for (const owner of readdirSync(warheadHf, { withFileTypes: true })) {
            if (!owner.isDirectory())
                continue;
            for (const repo of readdirSync(join(warheadHf, owner.name), { withFileTypes: true })) {
                if (!repo.isDirectory())
                    continue;
                const directory = join(warheadHf, owner.name, repo.name);
                for (const file of listGgufFiles(directory))
                    add(`gguf:${file}`);
                if (isTextCheckpoint(directory) && hasWeights(directory))
                    add(`mlx:${directory}`);
            }
        }
    }
    catch { /* no pulled native models yet */ }
    for (const cache of huggingFaceCacheRoots()) {
        let repos;
        try {
            repos = readdirSync(cache, { withFileTypes: true });
        }
        catch {
            continue;
        }
        for (const repo of repos) {
            if (!repo.isDirectory() || !repo.name.startsWith('models--'))
                continue;
            for (const directory of checkpointDirectories(join(cache, repo.name))) {
                for (const file of listGgufFiles(directory))
                    add(`gguf:${file}`);
                if (isTextCheckpoint(directory) && hasWeights(directory))
                    add(`mlx:${directory}`);
            }
        }
    }
    return found.sort();
}
/** Every locally loadable reference: Ollama stores first (they are what the
 *  daemon serves), then loose native weights. */
export function listLocalModels() {
    return [...listLocalOllamaModels(), ...listLocalNativeModels()];
}
/** True when a reference points at a path inside a model store, so callers can
 *  shorten it for display without hiding where it came from. */
export function isModelStorePath(path) {
    const roots = [...ollamaModelRoots(), join(warheadModelsRoot(), 'hf'), ...huggingFaceCacheRoots()];
    return roots.some(root => path === root || path.startsWith(root + sep));
}
