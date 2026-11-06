/**
 * bash — run a shell command.
 *
 * The one tool that can do anything, so it is also the one the gate segments
 * most carefully (see perm/gate.ts). Two implementation details matter:
 *
 * 1. The child is spawned in its OWN process group (`detached: true`) and the
 *    whole group is signalled on timeout. Killing only the direct child leaves
 *    its grandchildren running — a build or test runner routinely spawns
 *    children, and an orphaned `npm test` holding the terminal is a real hazard.
 *
 * 2. Output is truncated from the middle, not the end. The interesting part of
 *    a long log is usually the tail (the error), but the head carries the
 *    command's own output; keeping both ends beats keeping either.
 */
import { spawn } from 'node:child_process';
import { reqString, optInt, ArgError } from './args.js';
const DEFAULT_TIMEOUT_MS = 120_000;
const MAX_OUTPUT_BYTES = 30_000;
const HEAD_FRACTION = 0.4;
/** Assemble the head and tail buffers. Whether anything was dropped is tracked
 *  explicitly by the caller rather than inferred by comparing buffers. */
function join2(head, tail, dropped) {
    if (dropped === 0)
        return { text: head + tail, truncated: false };
    return {
        text: `${head}\n\n… [${dropped} characters omitted] …\n\n${tail}`,
        truncated: true,
    };
}
export const bash = {
    mutates: true,
    def: {
        name: 'bash',
        description: 'Run a shell command and return its output. Runs via `sh -c` in the working ' +
            'directory. Long output is truncated in the middle. Prefer a dedicated tool ' +
            'where one covers the task — use this for git, build and test commands, and ' +
            'for filesystem operations no tool provides. Quote paths containing spaces. ' +
            'Commands may require the user\'s approval.',
        parameters: {
            type: 'object',
            properties: {
                command: { type: 'string', description: 'The shell command to run' },
                timeout_ms: {
                    type: 'integer',
                    description: `Kill the command after this many milliseconds. Defaults to ${DEFAULT_TIMEOUT_MS}.`,
                },
            },
            required: ['command'],
        },
    },
    async run(args, ctx) {
        let command;
        let timeoutMs;
        try {
            command = reqString(args, 'command');
            timeoutMs = optInt(args, 'timeout_ms', 1) ?? ctx.config().limits.bashTimeoutMs ?? DEFAULT_TIMEOUT_MS;
        }
        catch (err) {
            return { ok: false, output: '', error: err instanceof ArgError ? err.message : String(err) };
        }
        if (ctx.signal.aborted) {
            return { ok: false, output: '', error: 'Cancelled before the command ran.' };
        }
        return new Promise(resolve => {
            let child;
            try {
                child = spawn('sh', ['-c', command], {
                    cwd: ctx.cwd,
                    detached: true, // own process group, so the whole tree can be signalled
                    stdio: ['ignore', 'pipe', 'pipe'],
                    env: { ...process.env },
                });
            }
            catch (err) {
                resolve({ ok: false, output: '', error: `Failed to spawn: ${err.message}` });
                return;
            }
            // Two buffers rather than one: the head is appended to until full, the
            // tail is a fixed-size ring. Appending to a single string and stopping at
            // the cap would discard the tail — the half most likely to hold the error.
            const HEAD_CAP = MAX_OUTPUT_BYTES;
            const TAIL_CAP = MAX_OUTPUT_BYTES;
            let stdoutHead = '';
            let stdoutTail = '';
            let stdoutDropped = 0;
            let stderrHead = '';
            let stderrTail = '';
            let stderrDropped = 0;
            // Returns the updated buffers plus the number of bytes discarded, which is
            // the only honest way to report truncation.
            const feed = (head, tail, chunk, cap, droppedSoFar) => {
                let h = head;
                let t = tail;
                let dropped = droppedSoFar;
                if (h.length < cap) {
                    const room = cap - h.length;
                    h += chunk.slice(0, room);
                    if (chunk.length > room) {
                        dropped += 0; // nothing lost yet; the overflow is still in `t`
                        t += chunk.slice(room);
                    }
                }
                else {
                    dropped += chunk.length;
                    t += chunk;
                }
                if (t.length > cap) {
                    dropped += t.length - cap;
                    t = t.slice(-cap);
                }
                return [h, t, dropped];
            };
            let settled = false;
            let timedOut = false;
            let cancelled = false;
            let escalation;
            const killTree = (signal) => {
                if (child.pid == null)
                    return;
                try {
                    // Negative pid targets the group created by detached:true.
                    process.kill(-child.pid, signal);
                }
                catch {
                    try {
                        child.kill(signal);
                    }
                    catch {
                        /* already gone */
                    }
                }
            };
            const finish = (code, spawnSignal) => {
                if (settled)
                    return;
                settled = true;
                if (timedOut || cancelled)
                    killTree('SIGKILL');
                clearTimeout(timer);
                clearTimeout(escalation);
                ctx.signal.removeEventListener('abort', onAbort);
                const o = join2(stdoutHead, stdoutTail, stdoutDropped);
                const e = join2(stderrHead, stderrTail, stderrDropped);
                const sections = [];
                if (o.text.trim() !== '')
                    sections.push(o.text);
                if (e.text.trim() !== '')
                    sections.push(`[stderr]\n${e.text}`);
                const meta = [];
                if (timedOut)
                    meta.push(`timed out after ${timeoutMs}ms`);
                if (cancelled)
                    meta.push('cancelled');
                if (o.truncated || e.truncated)
                    meta.push('output truncated');
                if (code !== null)
                    meta.push(`exit ${code}`);
                if (spawnSignal)
                    meta.push(`signal ${spawnSignal}`);
                const body = sections.length > 0 ? sections.join('\n') : '(no output)';
                const ok = !timedOut && !cancelled && code === 0;
                resolve({
                    ok,
                    output: `${body}${meta.length > 0 ? `\n[${meta.join(', ')}]` : ''}`,
                    error: ok ? undefined : meta.join(', ') || `exit ${code}`,
                });
            };
            const timer = setTimeout(() => {
                timedOut = true;
                killTree('SIGTERM');
                // Escalate if the process ignores SIGTERM.
                escalation = setTimeout(() => killTree('SIGKILL'), 2000);
                escalation.unref();
            }, timeoutMs);
            const onAbort = () => {
                cancelled = true;
                killTree('SIGTERM');
                escalation = setTimeout(() => killTree('SIGKILL'), 2000);
                escalation.unref();
            };
            ctx.signal.addEventListener('abort', onAbort, { once: true });
            child.stdout?.setEncoding('utf8');
            child.stderr?.setEncoding('utf8');
            child.stdout?.on('data', (d) => {
                const [h, tl, dr] = feed(stdoutHead, stdoutTail, d, HEAD_CAP, stdoutDropped);
                stdoutHead = h;
                stdoutTail = tl;
                stdoutDropped = dr;
            });
            child.stderr?.on('data', (d) => {
                const [h, tl, dr] = feed(stderrHead, stderrTail, d, Math.floor(MAX_OUTPUT_BYTES / 2), stderrDropped);
                stderrHead = h;
                stderrTail = tl;
                stderrDropped = dr;
            });
            child.on('error', err => {
                if (settled)
                    return;
                settled = true;
                clearTimeout(timer);
                clearTimeout(escalation);
                ctx.signal.removeEventListener('abort', onAbort);
                resolve({ ok: false, output: '', error: `Command failed to run: ${err.message}` });
            });
            child.on('close', finish);
        });
    },
};
