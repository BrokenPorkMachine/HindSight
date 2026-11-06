import { MAX_AUTO_CONTEXT_TOKENS } from '../model/modelfile.js';
export function inputBudgetChars(advertised) {
    const tokens = Math.min(advertised ?? MAX_AUTO_CONTEXT_TOKENS, MAX_AUTO_CONTEXT_TOKENS);
    const reply = Math.min(1024, Math.max(256, Math.floor(tokens / 4)));
    return Math.max(0, tokens - reply) * 4;
}
/** Includes message framing, tool-call arguments, and tool schemas. Token counts
 * remain estimates; the server's explicit overflow rejection stays enabled. */
export function requestChars(messages, tools) {
    return JSON.stringify({ messages, tools }).length;
}
export class ContextBudgetError extends Error {
    constructor() { super('Request exceeds the estimated context budget. Attach a smaller focused excerpt, reduce enabled tools, or start a new session with /clear.'); }
}
export function assertRequestFits(messages, tools, advertised) {
    if (requestChars(messages, tools) > inputBudgetChars(advertised))
        throw new ContextBudgetError();
}
