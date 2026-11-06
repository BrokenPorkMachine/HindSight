/** Direct access to Ollama's model-management API. */
import { providerForModel, authHeaders } from './provider.js';
import { clearModelSystemPromptCache } from './modelfile.js';
import { inferenceFetch } from './inference-transport.js';
function checkResult(value) {
    if (value && typeof value === 'object' && 'error' in value && typeof value.error === 'string' && value.error) {
        throw new Error(`Ollama: ${value.error}`);
    }
    return value;
}
async function request(path, body, model = '', method = 'POST') {
    const provider = providerForModel(model);
    const res = await inferenceFetch(`${provider.host}/api/${path}`, {
        method,
        headers: { ...authHeaders(provider), ...(body ? { 'content-type': 'application/json' } : {}) },
        ...(body ? { body: JSON.stringify(body) } : {}),
        signal: AbortSignal.timeout(120_000),
    });
    if (!res.ok)
        throw new Error(`Ollama HTTP ${res.status}: ${(await res.text()).slice(0, 300)}`);
    const text = await res.text();
    if (!text.trim())
        return {};
    try {
        return checkResult(JSON.parse(text));
    }
    catch {
        // /api/pull and /api/create stream newline-delimited JSON. Preserve the
        // final status for the CLI without requiring a second transport path.
        const lines = text.trim().split('\n').reverse();
        for (const line of lines) {
            try {
                return checkResult(JSON.parse(line));
            }
            catch (err) {
                if (err instanceof Error && err.message.startsWith('Ollama:'))
                    throw err;
                /* inspect the next frame */
            }
        }
        return { output: text.slice(-2000) };
    }
}
export const showModel = (model) => request('show', { name: model }, model);
async function mutate(path, body, model, method = 'POST') {
    const result = await request(path, body, model, method);
    clearModelSystemPromptCache();
    return result;
}
export const pullModel = (model) => mutate('pull', { model }, model);
export const deleteModel = (model) => mutate('delete', { model }, model, 'DELETE');
export const copyModel = (source, destination) => mutate('copy', { source, destination }, source);
export async function createModel(name, specification) {
    let definition;
    try {
        definition = JSON.parse(specification);
    }
    catch {
        throw new Error('model creation requires a JSON specification, e.g. {"from":"llama3.2:latest","system":"Be concise."}; raw Modelfile text is not supported by this API');
    }
    if (!definition || typeof definition !== 'object' || Array.isArray(definition))
        throw new Error('model creation specification must be a JSON object');
    const spec = definition;
    const from = typeof spec['from'] === 'string' && spec['from'].trim().length > 0;
    const files = spec['files'];
    const hasFiles = files && typeof files === 'object' && !Array.isArray(files) && Object.keys(files).length > 0;
    if (!from && !hasFiles)
        throw new Error('model creation requires "from" or "files" referencing blobs already uploaded to the Ollama server');
    return mutate('create', { ...spec, model: name, stream: false }, name);
}
/** Ollama's non-chat generation and embedding endpoints, kept here so callers
 * can use the native API without bypassing provider routing or auth. */
export const generate = (model, prompt, options) => request('generate', { model, prompt, stream: false, ...(options ? { options } : {}) }, model);
export const embed = (model, input) => request('embed', { model, input }, model);
export const version = () => request('version', undefined, '', 'GET');
export const runningModels = async () => {
    const result = await request('ps', undefined, '', 'GET');
    return Array.isArray(result.models) ? result.models : [];
};
// Re-export context-length helpers so callers can reach them from either module.
export { fetchModelContextLength, fetchModelContextSize, fetchModelContextWindow, getModelContextLength, modelContextLength, contextLength, extractContextLength, clearModelContextCache, } from './modelfile.js';
