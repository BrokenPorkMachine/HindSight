/**
 * Context compaction.
 *
 * Only ever runs at a TURN BOUNDARY, never mid-turn. Mid-turn compaction would
 * have to drop or synthesize tool results, and an orphaned `tool` message — one
 * whose preceding assistant `tool_calls` entry was removed — makes the next
 * request malformed. Even at a turn boundary the retained slice must start
 * outside a tool exchange: a message count alone does not guarantee that.
 *
 * The summary is injected as a user-role message rather than a system one. It is
 * a record of what happened, not an instruction, and keeping it in the user role
 * avoids competing with the real system prompt for authority.
 */
import { chatOnce } from '../model/client.js';
export function approxTokens(chars) {
    return Math.ceil(chars / 4);
}
export function conversationChars(messages) {
    let n = 0;
    for (const m of messages) {
        n += m.content.length;
        for (const c of m.toolCalls ?? [])
            n += JSON.stringify(c.args).length + c.name.length;
    }
    return n;
}
const SUMMARY_INSTRUCTIONS = `You are compacting an ongoing coding session so it can continue with less history.

Write a dense factual summary covering, in this order:
1. The user's goal and any explicit constraints they stated.
2. What has been established: files read, files changed, commands run and their outcomes.
3. Current state: what works, what is unfinished, what failed and why.
4. The immediate next step.

Rules:
- Be specific. Cite file paths and identifiers. Never write "the code" or "some files".
- Preserve exact error messages and command output that still matter.
- Do not invent anything. If something was not established, do not claim it.
- Do not address the reader. No preamble, no summary-of-summary. Plain factual notes.
- Aim for brevity over completeness, but do not drop a file path or an error string.`;
/** Render messages compactly for the summarizer. */
function renderForSummary(messages) {
    const lines = [];
    for (const m of messages) {
        if (m.role === 'system')
            continue;
        if (m.role === 'user') {
            lines.push(`USER: ${m.content}`);
        }
        else if (m.role === 'assistant') {
            if (m.content.trim() !== '')
                lines.push(`ASSISTANT: ${m.content}`);
            for (const c of m.toolCalls ?? []) {
                lines.push(`ASSISTANT called ${c.name}(${JSON.stringify(c.args).slice(0, 300)})`);
            }
        }
        else {
            const out = m.content.length > 1200 ? `${m.content.slice(0, 1200)} …[truncated]` : m.content;
            lines.push(`RESULT ${m.toolName ?? ''}: ${out}`);
        }
    }
    return lines.join('\n');
}
export async function compactIfNeeded(messages, opts) {
    const charsBefore = conversationChars(messages);
    const budget = opts.thresholdChars - (opts.reservedChars ?? 0);
    if (charsBefore <= budget) {
        return { messages, compacted: false, charsBefore, charsAfter: charsBefore };
    }
    const system = messages.find(m => m.role === 'system');
    const body = messages.filter(m => m.role !== 'system');
    if (body.length <= opts.keepRecent) {
        return {
            messages,
            compacted: false,
            charsBefore,
            charsAfter: charsBefore,
            error: 'nothing left to compact',
        };
    }
    let cut = Math.max(0, body.length - opts.keepRecent);
    // A retained result needs its assistant parent and all sibling results.
    while (cut > 0 && body[cut]?.role === 'tool')
        cut--;
    if (cut === 0)
        return { messages, compacted: false, charsBefore, charsAfter: charsBefore, error: 'nothing left to compact without splitting a tool exchange' };
    const older = body.slice(0, cut);
    const recent = body.slice(cut);
    const transcript = renderForSummary(older);
    if (transcript.trim() === '') {
        return { messages, compacted: false, charsBefore, charsAfter: charsBefore };
    }
    let summary;
    try {
        // A history that overflowed the working model cannot be sent wholesale to
        // the same model as a summarization request. Summarize bounded chunks first.
        const chunkChars = Math.max(512, ((opts.numCtx ?? 8192) - 2048) * 4 - SUMMARY_INSTRUCTIONS.length - 512);
        if (transcript.length > chunkChars * 32)
            throw new Error('history exceeds the bounded summarization budget; save the session and start a focused conversation');
        const summarize = async (text) => {
            const { content } = await chatOnce(opts.model, [{ role: 'system', content: SUMMARY_INSTRUCTIONS }, { role: 'user', content: text }], { numPredict: 1200, ...(opts.numCtx != null ? { numCtx: opts.numCtx } : {}), ...(opts.signal ? { signal: opts.signal } : {}) });
            if (!content.trim())
                throw new Error('summarizer returned nothing');
            return content.trim();
        };
        let work = transcript;
        let passes = 0;
        while (work.length > chunkChars) {
            if (++passes > 4)
                throw new Error('summarizer failed to reduce history within its context window');
            const chunks = [];
            for (let offset = 0; offset < work.length; offset += chunkChars) {
                opts.signal?.throwIfAborted();
                chunks.push(await summarize(work.slice(offset, offset + chunkChars)));
            }
            const combined = chunks.join('\n\n');
            if (combined.length >= work.length)
                throw new Error('chunk summaries were not smaller; keeping full history');
            work = combined;
        }
        summary = await summarize(work);
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        return { messages, compacted: false, charsBefore, charsAfter: charsBefore, error: msg };
    }
    if (summary === '') {
        return {
            messages,
            compacted: false,
            charsBefore,
            charsAfter: charsBefore,
            error: 'summarizer returned nothing',
        };
    }
    const note = {
        role: 'user',
        content: `[compacted: ${older.length} earlier message(s) summarized to preserve context]\n\n${summary}`,
    };
    const rebuilt = [...(system ? [system] : []), note, ...recent];
    const charsAfter = conversationChars(rebuilt);
    // A summary can be longer than what it replaced — short exchanges compress
    // badly. Keeping the original in that case is strictly better, since the point
    // of compaction is to reduce size, and the next turn will exceed the threshold
    // again anyway once more history accumulates.
    if (charsAfter >= charsBefore) {
        return {
            messages,
            compacted: false,
            charsBefore,
            charsAfter: charsBefore,
            error: `summary was not smaller (${charsBefore} -> ${charsAfter} chars); keeping full history`,
        };
    }
    return { messages: rebuilt, compacted: true, summary, charsBefore, charsAfter };
}
