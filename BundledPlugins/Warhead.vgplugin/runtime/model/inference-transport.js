/**
 * Model HTTP transport for the isolated worker.
 *
 * The Bubblewrap worker profile has no host network. When the trusted parent
 * supplies a scoped inference socket, every model HTTP request is routed over
 * that Unix socket. The parent proxy injects the real provider credential; the
 * sandbox only holds a short-lived task token. Outside the sandbox these
 * variables are absent, so the ordinary undici transport is used unchanged.
 */
import { Agent, Headers, fetch as undiciFetch } from 'undici';
const socketPath = process.env['WARHEAD_INFERENCE_SOCKET'];
const token = process.env['WARHEAD_INFERENCE_TOKEN'];
const enabled = Boolean(socketPath && token);
const IDLE_TIMEOUT_MS = 15 * 60 * 1000;
export const inferenceDispatcher = enabled
    ? new Agent({
        connect: { socketPath: socketPath },
        headersTimeout: IDLE_TIMEOUT_MS,
        bodyTimeout: IDLE_TIMEOUT_MS,
    })
    : undefined;
const proxyHeaders = enabled ? { authorization: `Bearer ${token}` } : {};
/** `undici.fetch` with the scoped proxy token added when isolation is active. */
export function inferenceFetch(input, init) {
    if (!enabled)
        return undiciFetch(input, init);
    const headers = new Headers(init?.headers);
    for (const [name, value] of Object.entries(proxyHeaders))
        headers.set(name, value);
    return undiciFetch(input, { ...init, dispatcher: inferenceDispatcher, headers });
}
