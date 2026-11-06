export const MAX_ATTACHMENT_BYTES = 8 * 1024 * 1024;
/** Validate the whole save before replacing any live conversation state. */
export function parseSession(value) {
    if (!value || typeof value !== 'object' || Array.isArray(value))
        throw new Error('invalid session file');
    const saved = value;
    if (saved['version'] !== undefined && saved['version'] !== 1 && saved['version'] !== 2)
        throw new Error('unsupported session version');
    const history = saved['history'];
    if (!Array.isArray(history))
        throw new Error('invalid session history');
    const pending = new Map();
    for (const m of history) {
        if (!m || typeof m.content !== 'string' || !['user', 'assistant', 'tool'].includes(m.role))
            throw new Error('invalid session message');
        if (m.role !== 'tool' && pending.size > 0)
            throw new Error('session is missing tool results');
        if (m.toolCalls !== undefined) {
            if (m.role !== 'assistant' || !Array.isArray(m.toolCalls))
                throw new Error('invalid session tool calls');
            for (const c of m.toolCalls) {
                if (!c || typeof c.id !== 'string' || !c.id || typeof c.name !== 'string' || !c.name || !c.args || typeof c.args !== 'object' || Array.isArray(c.args) || pending.has(c.id))
                    throw new Error('invalid session tool call');
                pending.set(c.id, c.name);
            }
        }
        if (m.role === 'tool') {
            if (typeof m.toolCallId !== 'string' || typeof m.toolName !== 'string' || pending.get(m.toolCallId) !== m.toolName)
                throw new Error('session contains an unmatched tool result');
            pending.delete(m.toolCallId);
        }
        else if (m.toolCallId !== undefined || m.toolName !== undefined)
            throw new Error('invalid session message');
    }
    if (pending.size > 0)
        throw new Error('session is missing tool results');
    const attachments = (saved['attachments'] ?? []);
    if (!Array.isArray(attachments) || attachments.some(a => !a || typeof a.name !== 'string' || typeof a.content !== 'string' || !Number.isSafeInteger(a.bytes) || a.bytes < 0 ||
        (a.kind !== undefined && !['text', 'source', 'binary', 'metadata'].includes(a.kind)) ||
        (a.language !== undefined && typeof a.language !== 'string') || (a.detectedType !== undefined && typeof a.detectedType !== 'string') ||
        Buffer.byteLength(a.content) > MAX_ATTACHMENT_BYTES ||
        (a.kind !== 'binary' && a.kind !== 'metadata' && a.bytes !== Buffer.byteLength(a.content))))
        throw new Error('invalid attachment data');
    return { history, attachments, verboseOutput: saved['verboseOutput'] === true };
}
