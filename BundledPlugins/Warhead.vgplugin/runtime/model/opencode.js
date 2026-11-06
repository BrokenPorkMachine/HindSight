/**
 * OpenCode Console transport for Warhead.
 *
 * OpenCode translates supported text models between API families. For this
 * first increment we use its OpenAI-compatible Chat Completions surface for
 * all explicitly selected opencode:<id> or opencode-go:<id> references.
 *
 * Keep native Ollama, MLX, GGUF, vLLM, tool permissions and goal supervision
 * unchanged. Never store or log API credentials.
 */
import { randomUUID } from 'node:crypto';
const SESSION_ID = 'ses_' + randomUUID();
const PAYG = 'https://opencode.ai/inference';
const GO = 'https://opencode.ai/inference/go';
const MAX_FRAME = 2 * 1024 * 1024;
const MAX_JSON = 4 * 1024 * 1024;
export class OpenCodeError extends Error {
    status;
    transient;
    constructor(message, status, transient = false) {
        super(message);
        this.name = 'OpenCodeError';
        this.status = status;
        this.transient = transient || status === 429 || (status !== undefined && status >= 500);
    }
}
export function isOpenCodeModel(model) {
    return model.startsWith('opencode:') || model.startsWith('opencode-go:');
}
export function parseOpenCodeModel(model) {
    const plan = model.startsWith('opencode-go:') ? 'go' : 'payg';
    if (!isOpenCodeModel(model))
        throw new OpenCodeError('not an OpenCode model reference');
    const id = model.slice(plan === 'go' ? 'opencode-go:'.length : 'opencode:'.length);
    if (!/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/.test(id)) {
        throw new OpenCodeError('invalid OpenCode model ID');
    }
    return { id, plan };
}
function baseURL(plan) {
    // Testing/private development may point to loopback ONLY; never allow an
    // untrusted remote override to receive a user's bearer token.
    const override = process.env['WARHEAD_OPENCODE_TEST_BASE_URL'];
    if (override) {
        const url = new URL(override);
        if (url.protocol !== 'http:' || !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) ||
            url.username || url.password || url.search || url.hash) {
            throw new OpenCodeError('OpenCode test endpoint must be unauthenticated loopback HTTP');
        }
        return url.href.replace(/\/+$/, '');
    }
    return plan === 'go' ? GO : PAYG;
}
export function openCodeEndpoint(model) {
    const selected = parseOpenCodeModel(model);
    return baseURL(selected.plan) + '/openai/v1/chat/completions';
}
function token() {
    const key = process.env['OPENCODE_API_KEY']?.trim();
    if (!key)
        throw new OpenCodeError('OpenCode requires OPENCODE_API_KEY; configure it locally, never in source');
    return key;
}
function wireMessages(messages) {
    return messages.map(m => {
        if (m.role === 'tool') {
            if (!m.toolCallId)
                throw new OpenCodeError('tool result lacks tool_call_id');
            return { role: 'tool', tool_call_id: m.toolCallId, content: m.content };
        }
        if (m.role === 'assistant' && m.toolCalls?.length) {
            return {
                role: 'assistant',
                content: m.content || null,
                tool_calls: m.toolCalls.map(c => ({
                    type: 'function', id: c.id,
                    function: { name: c.name, arguments: JSON.stringify(c.args) },
                })),
            };
        }
        return { role: m.role, content: m.content };
    });
}
function wireTools(tools) {
    return tools.map(t => ({
        type: 'function',
        function: { name: t.name, description: t.description, parameters: t.parameters },
    }));
}
function wireBody(model, messages, opts, stream) {
    return {
        model: parseOpenCodeModel(model).id,
        messages: wireMessages(messages),
        stream,
        ...(stream ? { stream_options: { include_usage: true } } : {}),
        ...(opts.tools?.length ? { tools: wireTools(opts.tools) } : {}),
        ...(opts.numPredict != null ? { max_completion_tokens: opts.numPredict } : {}),
        ...(opts.format === 'json' ? { response_format: { type: 'json_object' } } : {}),
    };
}
function parseArguments(raw) {
    let parsed = raw;
    if (typeof raw === 'string') {
        try {
            parsed = JSON.parse(raw || '{}');
        }
        catch {
            throw new OpenCodeError('invalid JSON in model tool arguments');
        }
    }
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        throw new OpenCodeError('model tool arguments must be a JSON object');
    }
    return parsed;
}
function normalizeCalls(calls) {
    if (!Array.isArray(calls))
        return [];
    return calls.map((call, i) => {
        if (!call.function?.name)
            throw new OpenCodeError('tool call has no function name');
        return {
            id: call.id || 'call_' + (i + 1),
            name: call.function.name,
            args: parseArguments(call.function.arguments ?? '{}'),
        };
    });
}
async function request(model, messages, opts, stream) {
    const key = token();
    let response;
    try {
        response = await fetch(openCodeEndpoint(model), {
            method: 'POST',
            headers: {
                authorization: 'Bearer ' + key,
                'content-type': 'application/json',
                accept: stream ? 'text/event-stream' : 'application/json',
                'x-opencode-session': process.env['WARHEAD_OPENCODE_SESSION_ID'] || SESSION_ID,
            },
            body: JSON.stringify(wireBody(model, messages, opts, stream)),
            ...(opts.signal ? { signal: opts.signal } : {}),
        });
    }
    catch (error) {
        if (opts.signal?.aborted)
            throw error;
        throw new OpenCodeError('OpenCode transport failed: ' + (error instanceof Error ? error.name : 'unknown'), undefined, true);
    }
    if (!response.ok) {
        // Never reflect untrusted response bodies that may contain credentials.
        await response.body?.cancel().catch(() => { });
        const hint = response.status === 401 ? 'invalid or expired key'
            : response.status === 403 ? 'model or plan not entitled for API access'
                : response.status === 404 ? 'model or endpoint not found'
                    : response.status === 429 ? 'rate or spending limit reached'
                        : 'request failed';
        throw new OpenCodeError('OpenCode HTTP ' + response.status + ': ' + hint, response.status);
    }
    return response;
}
export async function openCodeChatOnce(model, messages, opts = {}) {
    const response = await request(model, messages, opts, false);
    const raw = await response.text();
    if (raw.length > MAX_JSON)
        throw new OpenCodeError('OpenCode response exceeds limit');
    let json;
    try {
        json = JSON.parse(raw);
    }
    catch {
        throw new OpenCodeError('OpenCode returned invalid completion JSON');
    }
    const message = json?.choices?.[0]?.message;
    if (!message)
        throw new OpenCodeError('OpenCode completion did not contain a message');
    return {
        content: typeof message.content === 'string' ? message.content : '',
        toolCalls: normalizeCalls(message.tool_calls),
    };
}
function frameEvents(value, pending) {
    if (value?.error)
        throw new OpenCodeError('OpenCode returned a model error');
    const choice = value?.choices?.[0];
    const events = [];
    if (typeof choice?.delta?.content === 'string' && choice.delta.content) {
        events.push({ type: 'text', delta: choice.delta.content });
    }
    for (const part of choice?.delta?.tool_calls ?? []) {
        if (!Number.isSafeInteger(part.index) || part.index < 0 || part.index > 128) {
            throw new OpenCodeError('invalid streamed tool index');
        }
        const call = pending.get(part.index) ?? { id: '', name: '', arguments: '' };
        if (typeof part.id === 'string')
            call.id += part.id;
        if (typeof part.function?.name === 'string')
            call.name += part.function.name;
        if (typeof part.function?.arguments === 'string')
            call.arguments += part.function.arguments;
        if (call.arguments.length > MAX_FRAME)
            throw new OpenCodeError('streamed tool arguments exceed limit');
        pending.set(part.index, call);
    }
    if (choice?.finish_reason === 'tool_calls') {
        for (const [index, call] of [...pending.entries()].sort((a, b) => a[0] - b[0])) {
            if (!call.name)
                throw new OpenCodeError('incomplete streamed tool call');
            events.push({ type: 'tool_call', call: {
                    id: call.id || 'call_' + (index + 1),
                    name: call.name,
                    args: parseArguments(call.arguments),
                } });
        }
        pending.clear();
    }
    return events;
}
export async function* streamOpenCodeChat(model, messages, opts = {}) {
    const response = await request(model, messages, opts, true);
    if (!response.body)
        throw new OpenCodeError('OpenCode returned no streaming body');
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = '';
    let dataLines = [];
    let completed = false;
    let usage;
    const pending = new Map();
    let finishedReason = null;
    function consumeFrame() {
        if (!dataLines.length)
            return [];
        const data = dataLines.join('\n');
        dataLines = [];
        if (data === '[DONE]') {
            completed = true;
            if (pending.size)
                throw new OpenCodeError('OpenCode ended with incomplete tool calls');
            return [{ type: 'done', evalCount: usage?.completion_tokens ?? 0, promptEvalCount: usage?.prompt_tokens ?? 0 }];
        }
        let value;
        try {
            value = JSON.parse(data);
        }
        catch {
            throw new OpenCodeError('OpenCode returned invalid SSE JSON');
        }
        if (value?.usage)
            usage = value.usage;
        const result = frameEvents(value, pending);
        if (value?.choices?.[0]?.finish_reason)
            finishedReason = value.choices[0].finish_reason;
        return result;
    }
    try {
        while (true) {
            const { done, value } = await reader.read();
            if (done)
                break;
            buffer += value;
            if (buffer.length > MAX_FRAME)
                throw new OpenCodeError('OpenCode SSE frame exceeds limit');
            let index;
            while ((index = buffer.indexOf('\n')) >= 0) {
                const line = buffer.slice(0, index).replace(/\r$/, '');
                buffer = buffer.slice(index + 1);
                if (line === '') {
                    for (const event of consumeFrame())
                        yield event;
                }
                else if (line.startsWith('data:')) {
                    dataLines.push(line.slice(5).trimStart());
                }
            }
        }
        if (buffer.trim()) {
            if (buffer.startsWith('data:'))
                dataLines.push(buffer.slice(5).trim());
            else
                throw new OpenCodeError('OpenCode stream terminated mid-event', undefined, true);
        }
        if (dataLines.length)
            for (const event of consumeFrame())
                yield event;
        if (!completed) {
            // Strict completeness: a successful HTTP status alone is not completion.
            throw new OpenCodeError('OpenCode stream ended without [DONE] (' + (finishedReason ?? 'unknown') + ')', undefined, true);
        }
    }
    catch (error) {
        if (opts.signal?.aborted)
            return;
        throw error;
    }
    finally {
        await reader.cancel().catch(() => { });
        reader.releaseLock();
    }
}
