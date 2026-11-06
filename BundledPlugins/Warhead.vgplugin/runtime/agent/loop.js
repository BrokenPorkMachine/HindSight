/**
 * The agent loop — agentic behaviors enforced here, not in prompt:
 *  - max_steps ceiling (code-enforced, not prompt)
 *  - repeat call suppression + overlap-based read coverage (prevents loops)
 *  - transient upstream retry (2 attempts with exponential backoff)
 *  - concurrent read-only tools; batches with mutations run in order
 *  - permission gate (onToolStart per call; denial sticky for session)
 *  - compaction at turn boundaries (never mid-turn, to avoid orphaned tool messages)
 *
 * Model default system prompt is preserved ahead of agent framing so reasoning
 * protocols (Modelfile) are not lost. Agent framing is kept minimal; behavior
 * relies on code rules, not instruction injection.
 */
import { streamChat, isTransient } from '../model/client.js';
import { requestContextLength } from '../model/modelfile.js';
import { prepareTurn } from './turn.js';
import { assertRequestFits, ContextBudgetError, inputBudgetChars, requestChars } from '../context/budget.js';
import { compactIfNeeded } from '../context/compact.js';
import { utf8Prefix } from '../text.js';
import { appendFileSync } from 'node:fs';
import { adviceMessage, consultTeam } from './team.js';
import { expectsExecution, repeatedPlanningAction, unexecutedPlanningAction } from './planning-loop.js';
import { nativeToolsForTurn } from './native-tool-budget.js';
import { resolveLocalModel } from '../model/local-models.js';
export function traceEvent(msg) {
    if (process.env['WARHEAD_TRACE'])
        console.info(`[warhead] ${msg}`);
    try {
        const p = process.env['WARHEAD_TRACE_FILE'];
        if (p)
            appendFileSync(p, new Date().toISOString() + ' ' + msg + '\n');
    }
    catch {
        // ignore log-write failures
    }
}
/** How many times to retry a transient upstream failure within one turn. */
const TRANSIENT_RETRIES = 3;
/** A small model can mistake the mere presence of tools for an instruction to
 * use one. Keep unambiguous social turns conversational. Coding requests still
 * receive the full registry. */
export function isConversationalInput(input) {
    const t = input.trim();
    if (!t)
        return true;
    if (/^(?:hi|hello|hey|yo|greetings|good (?:morning|afternoon|evening)|how are you|what'?s up|why so shy|\?+)[.!?\s]*$/i.test(t))
        return true;
    // Very short thanks/gratitude
    if (/^(?:thanks|thank you|thx|ty|ok|okay|cool|nice|got it)[.!?\s]*$/i.test(t))
        return true;
    return false;
}
function sleep(ms, signal) {
    if (signal.aborted)
        return Promise.resolve();
    return new Promise(res => {
        const timer = setTimeout(() => {
            signal.removeEventListener('abort', onAbort);
            res();
        }, ms);
        const onAbort = () => {
            clearTimeout(timer);
            res();
        };
        signal.addEventListener('abort', onAbort, { once: true });
    });
}
/** Internal sentinel: never present a locally aborted completion as a provider failure. */
class PlanningLoopError extends Error {
    action;
    constructor(action) { super(`repeated unexecuted plan: ${action}`); this.action = action; }
}
/** Stable identity for repeat-call suppression. Deliberately ignores argument
 *  ordering noise but not values, so `read(a)` twice is caught while
 *  `read(a)` then `read(b)` is not. */
export function callSignature(call) {
    const keys = Object.keys(call.args).sort();
    const norm = keys.map(k => `${k}=${JSON.stringify(call.args[k], (_key, value) => value && typeof value === 'object' && !Array.isArray(value)
        ? Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b))) : value)}`).join(',');
    return `${call.name}(${norm})`;
}
function asInt(v) {
    if (typeof v === 'number' && Number.isFinite(v))
        return Math.trunc(v);
    if (typeof v === 'string') {
        const n = Number.parseInt(v, 10);
        if (Number.isFinite(n))
            return n;
    }
    return undefined;
}
/** Tracks which line ranges of which files have already been read.
 *
 *  Byte-identical detection is not enough for reads. Observed against
 *  gpt-oss-120b: after a failed edit it re-read the same region of the same file
 *  with the offset jittered by a line or two each time — read(a,44,20),
 *  read(a,45,20), read(a,46,20) — which sailed past an exact-match check and
 *  burned a dozen steps. Overlap-based coverage catches the whole family. */
export class ReadCoverage {
    seen = new Map();
    /** True when the requested range is already (almost) entirely covered. */
    isRedundant(path, offset, limit) {
        const ranges = this.seen.get(path);
        if (!ranges || ranges.length === 0)
            return false;
        const start = offset;
        const end = offset + Math.max(1, limit);
        const overlapping = ranges
            .map(([s, e]) => [Math.max(start, s), Math.min(end, e)])
            .filter(([s, e]) => s < e)
            .sort(([a], [b]) => a - b);
        let covered = 0;
        let through = start;
        for (const [s, e] of overlapping) {
            covered += Math.max(0, e - Math.max(s, through));
            through = Math.max(through, e);
        }
        return covered >= Math.max(1, Math.ceil((end - start) * 0.7));
    }
    record(path, offset, limit) {
        const ranges = this.seen.get(path) ?? [];
        ranges.push([offset, offset + Math.max(1, limit)]);
        // Keep the set bounded; only recent reads matter for loop detection.
        if (ranges.length > 15)
            ranges.splice(0, ranges.length - 15);
        this.seen.set(path, ranges);
    }
    reset() {
        this.seen.clear();
    }
}
function readWindow(call) {
    if (call.name !== 'read_file')
        return null;
    const path = call.args['path'];
    if (typeof path !== 'string' || path === '')
        return null;
    return {
        path,
        offset: asInt(call.args['offset']) ?? 1,
        limit: asInt(call.args['limit']) ?? 2000,
    };
}
/** Deadlines cancel cooperative tools and always release their timer/listener. */
async function dispatchTool(registry, call, ctx, signal, timeoutMs) {
    const controller = new AbortController();
    const combined = AbortSignal.any([signal, ctx.signal, controller.signal]);
    let timer;
    let onAbort = () => { };
    let cleanupTimer;
    let work;
    try {
        const aborted = new Promise((_, reject) => {
            onAbort = () => reject(combined.reason ?? new Error('cancelled'));
            combined.addEventListener('abort', onAbort, { once: true });
            if (combined.aborted)
                onAbort();
        });
        timer = setTimeout(() => controller.abort(new Error('Tool dispatch timed out')), timeoutMs);
        work = registry.dispatch(call.name, call.args, { ...ctx, signal: combined });
        return await Promise.race([work, aborted]);
    }
    catch (error) {
        // Cooperative operations get time to finish termination before another
        // mutation starts. An uncooperative plugin still cannot hold the CLI forever.
        if (combined.aborted && work)
            await Promise.race([work, new Promise(resolve => { cleanupTimer = setTimeout(resolve, 2500); })]);
        throw error;
    }
    finally {
        clearTimeout(timer);
        clearTimeout(cleanupTimer);
        combined.removeEventListener('abort', onAbort);
    }
}
export async function runTurn(opts) {
    const { model, registry, config, ctx, history, userText, signal } = opts;
    if (opts.maxOutputTokens !== undefined && (!Number.isSafeInteger(opts.maxOutputTokens) || opts.maxOutputTokens < 1)) {
        throw new TypeError('maxOutputTokens must be a positive safe integer');
    }
    const emit = opts.onEvent ?? (() => { });
    const conversational = isConversationalInput(userText);
    let prepared;
    try {
        prepared = await prepareTurn({ ...opts, conversational,
            onContext: (before, after, error) => emit({ type: 'context', before, after, ...(error ? { error } : {}) }),
        });
    }
    catch (error) {
        const reason = signal.aborted ? 'cancelled' : error instanceof ContextBudgetError ? 'context_limit' : 'transport_error';
        if (!signal.aborted)
            emit({ type: 'error', message: error instanceof Error ? error.message : String(error) });
        emit({ type: 'stop', reason, steps: 0 });
        return { stopReason: reason, steps: 0, messages: [...history] };
    }
    const { messages, modelContextTokens } = prepared;
    const teamMessage = conversational ? null : adviceMessage(await consultTeam(config, model, history, userText, signal, emit));
    if (teamMessage)
        messages.splice(messages.length - 1, 0, teamMessage);
    const finish = (stopReason, steps) => {
        const saved = messages.filter(message => message !== teamMessage);
        opts.onCheckpoint?.(saved);
        return { stopReason, steps, messages: saved };
    };
    const seen = new Map();
    let emptyRecoveryUsed = false;
    let planningRecoveryUsed = false;
    let repeatRecoveryUsed = false;
    // Explanatory questions can legitimately quote a repeated plan. Only force
    // action when the user requested execution and the registry has real tools.
    const executionExpected = !conversational && registry.definitions().length > 0 && expectsExecution(userText);
    const coverage = new ReadCoverage();
    const maxSteps = Math.max(1, config.limits.maxSteps);
    const outputBudget = opts.maxOutputTokens;
    let outputTokens = 0;
    for (let step = 1; step <= maxSteps; step++) {
        if (signal.aborted) {
            emit({ type: 'stop', reason: 'cancelled', steps: step - 1 });
            return finish('cancelled', step - 1);
        }
        if (outputBudget !== undefined && outputTokens >= outputBudget) {
            emit({ type: 'stop', reason: 'budget_exhausted', steps: step - 1 });
            return finish('budget_exhausted', step - 1);
        }
        // --- assistant turn -------------------------------------------------
        // A transient upstream failure (5xx, 429, dropped connection) is retried
        // rather than ending the turn: losing a half-finished multi-step task to a
        // proxy blip is far worse than a short wait. Fatal errors are not retried,
        // since a 400 will just fail again.
        let text = '';
        const calls = [];
        let attempt = 0;
        let overflowRetried = false;
        let lastErr;
        // Match the tool surface used by prepareTurn and its context budget.
        // This changes only advertised schemas, never permission or dispatch.
        const source = resolveLocalModel(model);
        const narrowed = source && modelContextTokens !== null && modelContextTokens <= 8192
            ? nativeToolsForTurn(registry.tools, userText)
            : registry.tools;
        const tools = conversational ? [] : narrowed.map(tool => tool.def);
        try {
            assertRequestFits(messages, tools, modelContextTokens);
        }
        catch (error) {
            emit({ type: 'error', message: error instanceof Error ? error.message : String(error) });
            emit({ type: 'stop', reason: 'context_limit', steps: step - 1 });
            return finish('context_limit', step - 1);
        }
        const numCtx = requestContextLength(modelContextTokens, JSON.stringify({ messages, tools }).length);
        while (true) {
            text = '';
            calls.length = 0;
            // An independent abort interrupts a self-looping completion without
            // cancelling the user's entire task or the one permitted recovery.
            const completionAbort = new AbortController();
            const completionSignal = AbortSignal.any([signal, completionAbort.signal]);
            try {
                for await (const ev of streamChat(model, messages, {
                    tools,
                    signal: completionSignal,
                    numCtx,
                    ...(outputBudget !== undefined ? { numPredict: Math.max(1, outputBudget - outputTokens) } : {}),
                })) {
                    if (ev.type === 'text') {
                        text += ev.delta;
                        if (text.length > 2 * 1024 * 1024)
                            throw new Error('Model response exceeded the 2 MiB limit; request a narrower answer');
                        if (executionExpected && calls.length === 0) {
                            const action = repeatedPlanningAction(text);
                            if (action) {
                                completionAbort.abort(new Error('repetitive planning without tool execution'));
                                throw new PlanningLoopError(action);
                            }
                        }
                        emit({ type: 'text', delta: ev.delta });
                    }
                    else if (ev.type === 'tool_call') {
                        if (calls.length >= 64)
                            throw new Error('Model emitted more than 64 tool calls in one response; request a smaller batch');
                        // Ollama can repeat a name across frames while streaming; dedupe by id.
                        if (!calls.some(c => c.id === ev.call.id))
                            calls.push(ev.call);
                    }
                    else if (ev.type === 'done') {
                        outputTokens += Math.max(0, ev.evalCount);
                        emit({ type: 'usage', promptEvalCount: ev.promptEvalCount, evalCount: ev.evalCount });
                    }
                }
                lastErr = undefined;
                break;
            }
            catch (err) {
                // A locally detected planning cycle is not a network error and must
                // not enter the transient HTTP retry path.
                if (err instanceof PlanningLoopError) {
                    lastErr = err;
                    outputTokens += Math.ceil(text.length / 3); // interrupted streams lack usage telemetry
                    break;
                }
                if (signal.aborted) {
                    emit({ type: 'stop', reason: 'cancelled', steps: step - 1 });
                    return finish('cancelled', step - 1);
                }
                lastErr = err;
                // The daemon's tokenizer or actual context limit can differ from our
                // estimate. Only retry before any output, and retain the current user
                // request and every subsequent tool exchange intact.
                const detail = err instanceof Error ? err.message : (err && typeof err === "object" && "message" in err ? err.message : String(err));
                const overflow = /(?:exceeds the available context size|exceed_context_size_error)/i.test(detail);
                if (overflow && !overflowRetried && text.length === 0 && calls.length === 0) {
                    overflowRetried = true;
                    const body = messages.filter(m => m.role !== 'system');
                    const lastUser = body.findLastIndex(m => m.role === 'user');
                    const keepRecent = lastUser >= 0 ? body.length - lastUser : body.length;
                    const limit = detail.match(/(?:available context size|n_ctx[\\"\s:]+)(?:\s*\(?\s*)?(\d+)/i);
                    const actualCtx = limit ? Number(limit[1]) : numCtx;
                    const before = requestChars(messages, tools);
                    const result = await compactIfNeeded(messages, {
                        model,
                        thresholdChars: 0,
                        keepRecent,
                        signal,
                        numCtx: Math.min(numCtx, actualCtx),
                    });
                    if (result.compacted) {
                        messages.splice(0, messages.length, ...result.messages);
                        emit({ type: 'context', before, after: requestChars(messages, tools) });
                        continue;
                    }
                    if (result.error)
                        emit({ type: 'context', before, after: before, error: result.error });
                }
                // Retrying after streamed prose has been displayed would concatenate
                // two different completions in the UI and transcript.
                if (isTransient(err) && text.length === 0 && attempt < TRANSIENT_RETRIES) {
                    attempt++;
                    const backoff = 400 * 2 ** (attempt - 1);
                    emit({
                        type: 'error',
                        message: `transient model error, retrying in ${backoff}ms (${attempt}/${TRANSIENT_RETRIES}): ${err instanceof Error ? err.message : String(err)}`,
                    });
                    await sleep(backoff, signal);
                    continue;
                }
                break;
            }
        }
        if (signal.aborted) {
            emit({ type: 'stop', reason: 'cancelled', steps: step - 1 });
            return finish('cancelled', step - 1);
        }
        if (lastErr !== undefined && !(lastErr instanceof PlanningLoopError)) {
            const msg = lastErr instanceof Error ? lastErr.message : String(lastErr);
            if (text.length > 0) {
                messages.push({ role: 'assistant', content: `${text}\n\n[Response interrupted before completion.]` });
                emit({ type: 'error', message: 'response interrupted after partial output; use /retry to restart the prompt' });
            }
            emit({ type: 'error', message: msg });
            const reason = /(?:exceeds the available context size|exceed_context_size_error|prompt exceeds this GGUF model.s context limit)/i.test(msg) ? 'context_limit' : 'transport_error';
            emit({ type: 'stop', reason, steps: step - 1 });
            return finish(reason, step - 1);
        }
        if (calls.length === 0) {
            // A model that merely *announces* tool use has not executed the user's
            // request. Nudge once and require an actual tool call or substantive
            // completion; do not burn all maxSteps on self-replanning.
            const stalledAction = executionExpected
                ? (lastErr instanceof PlanningLoopError ? lastErr.action : repeatedPlanningAction(text) ?? unexecutedPlanningAction(text))
                : null;
            if (stalledAction) {
                const detail = `model repeatedly planned to use '${stalledAction}' without executing a tool`;
                messages.push({ role: 'assistant', content: text.slice(0, 512) });
                if (!planningRecoveryUsed && step < maxSteps && (outputBudget === undefined || outputTokens < outputBudget)) {
                    planningRecoveryUsed = true;
                    emit({ type: 'error', message: `${detail}; attempting one recovery` });
                    messages.push({ role: 'user', content: `Your previous completion only proposed an action (${stalledAction}); it did not execute one. Do not repeat intentions, self-corrections, or planning prose. If an authorized tool is available, issue its actual tool call now. Otherwise explain the precise blocker and stop. Never claim an action was performed unless a tool returned a result.` });
                    continue;
                }
                emit({ type: 'error', message: `${detail}; stopped after the bounded recovery attempt` });
                emit({ type: 'stop', reason: 'planning_loop', steps: step });
                return finish('planning_loop', step);
            }
            if (text.trim() === '') {
                // A reasoning model may consume its generation budget before producing
                // visible text. Retry once with explicit guidance; never loop forever.
                if (!emptyRecoveryUsed && step < maxSteps && (outputBudget === undefined || outputTokens < outputBudget)) {
                    emptyRecoveryUsed = true;
                    messages.push({ role: 'user', content: 'The previous completion contained no visible text or tool calls. Continue the current task now: issue a valid tool call or explain the next action in plain text. Do not repeat a command without new evidence.' });
                    continue;
                }
                emit({ type: 'error', message: 'model returned no text or tool calls after recovery (or output-token budget exhausted); check model tool capability, reasoning budget and context size' });
                emit({ type: 'stop', reason: 'empty_response', steps: step - 1 });
                return finish('empty_response', step - 1);
            }
            messages.push({ role: 'assistant', content: text });
            emit({ type: 'stop', reason: 'answered', steps: step - 1 });
            return finish('answered', step - 1);
        }
        messages.push({ role: 'assistant', content: text, toolCalls: calls });
        // Approval prompts share one terminal. Queue them even for read-only tools.
        let approvalQueue = Promise.resolve(true);
        const execute = async (call) => {
            if (signal.aborted) {
                return {
                    call,
                    outcome: { name: call.name, ok: false, output: 'cancelled', error: 'cancelled' },
                    denied: true,
                };
            }
            const win = readWindow(call);
            const redundantRead = win !== null && coverage.isRedundant(win.path, win.offset, win.limit);
            const sig = redundantRead ? `read-coverage:${win.path}` : callSignature(call);
            if (registry.get(call.name)?.mutates && opts.blockedMutations?.has(callSignature(call))) {
                return { call, outcome: { name: call.name, ok: false, output: 'This mutation was already attempted before the retry. Inspect the recorded result and current state before proposing another action. Use /retry --restart only to explicitly replay the original task.', error: 'mutation replay blocked' }, denied: true };
            }
            const prior = seen.get(sig) ?? 0;
            seen.set(sig, prior + 1);
            if (prior + 1 > config.limits.repeatCallLimit) {
                const msg = redundantRead
                    ? `You have already read this region of ${win.path} ${prior} time(s) with only the offset or limit changing slightly. The content is the same. Either you have what you need — use it — or you need something different: read a different file, read a much wider window, or ask the user what to do.`
                    : `Refusing to repeat '${call.name}' with identical arguments — this exact call has already been attempted ${prior} time(s). Change the arguments, use a different tool, or explain to the user what is blocking you.`;
                return {
                    call,
                    outcome: { name: call.name, ok: false, output: msg, error: msg },
                    denied: false,
                    suppressed: true,
                    msg,
                };
            }
            if (opts.onToolStart) {
                approvalQueue = approvalQueue.then(() => signal.aborted ? false : opts.onToolStart(call)).catch(error => {
                    emit({ type: 'error', message: `approval failed: ${error instanceof Error ? error.message : String(error)}` });
                    return false;
                });
                const allowed = await approvalQueue;
                if (!allowed) {
                    return {
                        call,
                        outcome: { name: call.name, ok: false, output: 'denied by user', error: 'denied by user' },
                        denied: true,
                    };
                }
            }
            if (signal.aborted)
                return { call, outcome: { name: call.name, ok: false, output: 'cancelled', error: 'cancelled' }, denied: true };
            emit({ type: 'tool_start', step, id: call.id, name: call.name, args: call.args });
            if (registry.get(call.name)?.mutates)
                opts.onCheckpoint?.(messages.filter(message => message !== teamMessage));
            const toolTimeout = config.limits.perToolTimeoutMs?.[call.name] ?? config.limits.toolTimeoutMs ?? 30_000;
            let successfulNonBashMutation = false;
            try {
                const outcome = await dispatchTool(registry, call, ctx, signal, toolTimeout);
                successfulNonBashMutation = outcome.ok && registry.get(call.name)?.mutates === true && call.name !== 'bash';
                if (win && outcome.ok)
                    coverage.record(win.path, win.offset, win.limit);
                emit({ type: 'tool_end', id: call.id, name: call.name, outcome });
                return { call, outcome, denied: false };
            }
            catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                emit({ type: 'tool_end', id: call.id, name: call.name, outcome: { name: call.name, ok: false, output: msg, error: msg } });
                return { call, outcome: { name: call.name, ok: false, output: msg, error: msg }, denied: false };
            }
            finally {
                // Even a failed or cancelled mutation may have changed a previous read.
                if (registry.get(call.name)?.mutates) {
                    coverage.reset();
                    for (const key of seen.keys())
                        if (key.startsWith('read_file(') || key.startsWith('read-coverage:'))
                            seen.delete(key);
                    // A successful edit changes the repository state. Re-running the same
                    // build/test bash command after that edit is meaningful, unlike
                    // repeating bash with no intervening change.
                    if (successfulNonBashMutation) {
                        for (const key of seen.keys())
                            if (key.startsWith('bash('))
                                seen.delete(key);
                    }
                }
            }
        };
        const toolResults = [];
        if (calls.some(call => registry.get(call.name)?.mutates)) {
            for (const call of calls)
                toolResults.push(await execute(call));
        }
        else {
            const ordered = new Array(calls.length);
            let next = 0;
            await Promise.all(Array.from({ length: Math.min(8, calls.length) }, async () => {
                while (next < calls.length) {
                    const index = next++;
                    ordered[index] = await execute(calls[index]);
                }
            }));
            toolResults.push(...ordered);
        }
        for (const res of toolResults) {
            if (res.denied || res.suppressed) {
                emit({
                    type: 'tool_end',
                    id: res.call.id,
                    name: res.call.name,
                    outcome: res.outcome,
                });
            }
            messages.push({
                role: 'tool',
                toolCallId: res.call.id,
                toolName: res.call.name,
                content: boundedResult(res.outcome.output || res.outcome.error || '(no output)', Math.max(256, Math.min(8192, Math.floor(inputBudgetChars(modelContextTokens) / Math.max(8, toolResults.length * 4))))),
            });
        }
        opts.onCheckpoint?.(messages.filter(message => message !== teamMessage));
        // Every call needs a result, including siblings of a suppressed call.
        // A duplicate request is not evidence that unrelated sibling work failed.
        // Give the model exactly one opportunity to use the results and choose a
        // DIFFERENT action; never re-execute the suppressed call on its behalf.
        if (signal.aborted) {
            emit({ type: 'stop', reason: 'cancelled', steps: step });
            return finish('cancelled', step);
        }
        const suppressed = toolResults.filter(res => res.suppressed);
        if (suppressed.length > 0) {
            if (!repeatRecoveryUsed && step < maxSteps &&
                (outputBudget === undefined || outputTokens < outputBudget)) {
                repeatRecoveryUsed = true;
                const names = [...new Set(suppressed.map(res => res.call.name))].slice(0, 8).join(', ');
                emit({ type: 'error', message: `suppressed redundant tool call(s): ${names}; allowing one corrective step` });
                messages.push({
                    role: 'user',
                    content: 'The previous tool batch included a redundant call that was NOT executed. ' +
                        'All returned tool results are authoritative, including results from other successful calls in that batch. ' +
                        'Do not retry the suppressed call with unchanged arguments or claim it succeeded. ' +
                        'Use existing results to progress with a DIFFERENT justified action, or explain the blocker and stop. ' +
                        'This is your only recovery opportunity for repeated calls in this turn.',
                });
                continue;
            }
            emit({ type: 'stop', reason: 'repeat_suppressed', steps: step });
            return finish('repeat_suppressed', step);
        }
    }
    messages.push({
        role: 'user',
        content: `You have used all ${maxSteps} tool-call steps for this turn. Summarize what you found ` +
            `and what remains, and do not call more tools.`,
    });
    emit({ type: 'stop', reason: 'max_steps', steps: maxSteps });
    return finish('max_steps', maxSteps);
}
/** Drop the system message and return the rest as persistable history. */
export function toHistory(messages) {
    return messages.filter(m => m.role !== 'system');
}
export function describeStop(r, steps) {
    switch (r) {
        case 'answered':
            return `answered after ${steps} tool step(s)`;
        case 'max_steps':
            return `stopped at the ${steps}-step ceiling`;
        case 'planning_loop':
            return 'stopped: repeated planning without executing a tool';
        case 'repeat_suppressed':
            // Also fires for overlap-detected re-reads, not only byte-identical calls.
            return `stopped: repeated tool call with no new information`;
        case 'cancelled':
            return 'cancelled';
        case 'transport_error':
            return 'stopped: model transport error';
        case 'empty_response':
            return 'stopped: model returned an empty response';
        case 'context_limit':
            return 'stopped: context budget reached; continue with a focused prompt';
        case 'budget_exhausted':
            return 'stopped: governed output-token budget reached';
    }
}
function boundedResult(output, maxBytes) {
    const prefix = utf8Prefix(output, maxBytes);
    return prefix.length === output.length ? output : `${prefix}\n[tool result clipped to fit context; narrow or page the request for more detail]`;
}
