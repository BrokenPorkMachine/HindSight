/**
 * Acceptance gates.
 *
 * A model saying "done" is evidence of a claim, never evidence of completion.
 * This module runs the commands and file checks a task or goal declared, records
 * exit codes and full logs, and distinguishes "failed" from "could not run"
 * (permission denied, missing tool, cancellation). Neither may be reported as
 * success.
 */
import { spawn } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { writePrivateText } from '../context/storage.js';
import { hashFile, resolveInside } from './repo.js';
const MAX_OUTPUT = 24_000;
const HEAD_FRACTION = 0.4;
function truncateMiddle(text) {
    if (text.length <= MAX_OUTPUT)
        return text;
    const head = Math.floor(MAX_OUTPUT * HEAD_FRACTION);
    const tail = MAX_OUTPUT - head;
    return `${text.slice(0, head)}\n\n… [${text.length - MAX_OUTPUT} characters omitted] …\n\n${text.slice(-tail)}`;
}
function safeLogName(command, index) {
    const slug = command.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 60) || 'command';
    return `${String(index + 1).padStart(2, '0')}-${slug}.log`;
}
function redactLog(text) {
    return text
        .replace(/(\b(?:authorization|api[_ -]?key|access[_ -]?token|secret|password)\b\s*[:=]\s*)([^\s,;]+)/ig, '$1[REDACTED]')
        .replace(/\bBearer\s+[A-Za-z0-9._~+/=-]+/ig, 'Bearer [REDACTED]');
}
function run(command, cwd, signal, timeoutMs) {
    return new Promise(resolvePromise => {
        if (signal.aborted) {
            resolvePromise({ exitCode: null, output: 'cancelled before the command ran', timedOut: false, blocked: true });
            return;
        }
        let child;
        try {
            child = spawn('sh', ['-c', command], {
                cwd, detached: true, stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env },
            });
        }
        catch (error) {
            resolvePromise({ exitCode: null, output: `failed to spawn: ${error instanceof Error ? error.message : String(error)}`, timedOut: false, blocked: true });
            return;
        }
        let stdout = '';
        let stderr = '';
        let timedOut = false;
        let settled = false;
        const finish = (exitCode) => {
            if (settled)
                return;
            settled = true;
            cleanup();
            resolvePromise({ exitCode, output: truncateMiddle([stdout, stderr].filter(Boolean).join('\n')), timedOut, blocked: false });
        };
        const kill = () => {
            try {
                if (child.pid)
                    process.kill(-child.pid, 'SIGKILL');
            }
            catch {
                try {
                    child.kill('SIGKILL');
                }
                catch { /* gone */ }
            }
        };
        const timer = setTimeout(() => { timedOut = true; kill(); }, Math.max(1, timeoutMs));
        const onAbort = () => { kill(); };
        const cleanup = () => {
            clearTimeout(timer);
            signal.removeEventListener('abort', onAbort);
        };
        signal.addEventListener('abort', onAbort, { once: true });
        child.stdout?.on('data', (chunk) => { if (stdout.length < 4 * MAX_OUTPUT)
            stdout += chunk.toString('utf8'); });
        child.stderr?.on('data', (chunk) => { if (stderr.length < 4 * MAX_OUTPUT)
            stderr += chunk.toString('utf8'); });
        child.on('error', error => {
            if (settled)
                return;
            settled = true;
            cleanup();
            resolvePromise({ exitCode: null, output: `failed to spawn: ${error.message}`, timedOut, blocked: true });
        });
        child.on('close', code => finish(code));
    });
}
export async function runVerification(input) {
    const results = [];
    const timeoutMs = Math.max(1_000, Math.min(input.timeoutMs ?? 10 * 60_000, 2_147_483_647));
    for (const [index, command] of input.commands.entries()) {
        const logPath = join(input.logDir, safeLogName(command, index));
        let authorized = false;
        if (!input.signal.aborted && input.authorize) {
            try {
                authorized = await input.authorize(command);
            }
            catch {
                authorized = false;
            }
        }
        if (!authorized) {
            const output = input.signal.aborted
                ? 'cancelled before verification'
                : 'verification command was not authorized by the active permission policy';
            writePrivateText(logPath, redactLog(`$ ${command}\n${output}\n`));
            results.push({ command, ok: false, exitCode: null, output, logPath, blocked: true, timedOut: false });
            continue;
        }
        const result = await run(command, input.workspace, input.signal, timeoutMs);
        writePrivateText(logPath, redactLog(`$ ${command}\n\nexit: ${result.exitCode ?? 'none'}${result.timedOut ? ' (timed out)' : ''}\n\n${result.output}\n`));
        results.push({
            command, ok: result.exitCode === 0 && !result.timedOut, exitCode: result.exitCode,
            output: result.output, logPath, blocked: result.blocked, timedOut: result.timedOut,
        });
    }
    const fileChecks = input.files.map(path => {
        const absolute = resolveInside(input.workspace, path);
        const exists = absolute !== null && existsSync(absolute) && statSync(absolute).isFile();
        return { path, exists, hash: absolute && exists ? hashFile(absolute) : null };
    });
    const blocked = results.some(result => result.blocked);
    const failed = results.some(result => !result.ok && !result.blocked) || fileChecks.some(check => !check.exists);
    const status = input.commands.length === 0 && input.files.length === 0
        ? 'unverified'
        : blocked && !failed ? 'blocked' : failed ? 'failed' : 'passed';
    const detail = [
        ...results.map(result => `$ ${result.command}\nexit ${result.exitCode ?? 'none'}${result.blocked ? ' (blocked)' : ''}${result.timedOut ? ' (timeout)' : ''}`),
        ...fileChecks.map(check => `${check.path}: ${check.exists ? `present (${check.hash?.slice(0, 12)})` : 'missing'}`),
    ].join('\n');
    return { status, results, fileChecks, detail: detail || 'no automated verification was configured' };
}
