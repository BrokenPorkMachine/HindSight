/** Private recovery state. Pending tool calls are restored as uncertain rather
 * than replayed: a crash cannot tell us whether an external mutation committed. */
import { lstatSync, readFileSync, readdirSync, statSync, unlinkSync } from 'node:fs';
import { dirname, isAbsolute, join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { transcriptDir } from '../config.js';
import { parseSession } from './session.js';
import { pruneRecovery } from './retention.js';
import { syncDirectory, writePrivateJson } from './storage.js';
import { digest, replaceProjectFile } from '../tools/files.js';
export function closePendingCalls(history, mutations = []) {
    const pending = new Map();
    const result = [];
    const close = () => {
        for (const call of pending.values()) {
            const outcome = mutations.findLast(record => record.call.id === call.id && record.call.name === call.name && JSON.stringify(record.call.args) === JSON.stringify(call.args))?.outcome;
            result.push({ role: 'tool', toolCallId: call.id, toolName: call.name,
                content: outcome ? outcome.output || outcome.error || '(no output)' : 'Execution was interrupted before a durable result was recorded. Outcome is unknown; inspect current state before attempting this operation again.' });
        }
        pending.clear();
    };
    for (const message of history) {
        if (message.role === 'system')
            continue;
        if (message.role !== 'tool')
            close();
        result.push(message);
        if (message.role === 'assistant')
            for (const call of message.toolCalls ?? [])
                pending.set(call.id, call);
        if (message.role === 'tool' && message.toolCallId)
            pending.delete(message.toolCallId);
    }
    close();
    return result;
}
export function recoveryRoot() { return join(transcriptDir(), '..', 'recovery'); }
export class RecoveryJournal {
    file;
    enabled;
    cwd;
    constructor(enabled, cwd, dir = join(recoveryRoot(), 'sessions')) {
        this.enabled = enabled;
        this.cwd = cwd;
        this.file = join(dir, `${Date.now()}-${randomUUID()}.json`);
    }
    save(state) {
        if (!this.enabled)
            return;
        const history = closePendingCalls(state.history, state.turn?.mutations);
        writePrivateJson(this.file, { version: 2, cwd: this.cwd, savedAt: new Date().toISOString(), ...state, history });
        pruneRecovery(dirname(this.file), this.file);
    }
    load(path = this.latest()) {
        if (!path)
            throw new Error('No recovery checkpoint found for this directory');
        const saved = readPrivateJson(path);
        if (saved['cwd'] !== this.cwd)
            throw new Error('Recovery checkpoint belongs to another working directory');
        const session = parseSession(saved);
        const rawTurn = saved['turn'];
        let turn = null;
        if (rawTurn) {
            if (typeof rawTurn.input !== 'string' || !Array.isArray(rawTurn.mutations))
                throw new Error('Invalid recovery turn');
            const before = parseSession({ history: rawTurn.before }).history;
            for (const record of rawTurn.mutations) {
                if (!record || !record.call)
                    throw new Error('Invalid recovery mutation');
                parseSession({ history: [{ role: 'assistant', content: '', toolCalls: [record.call] }, { role: 'tool', content: '', toolCallId: record.call.id, toolName: record.call.name }] });
                if (record.outcome && (typeof record.outcome.ok !== 'boolean' || typeof record.outcome.output !== 'string'))
                    throw new Error('Invalid recovery outcome');
            }
            turn = { before, input: rawTurn.input, mutations: rawTurn.mutations };
        }
        return { ...session, turn };
    }
    latest() {
        const dir = dirname(this.file);
        let names;
        try {
            names = readdirSync(dir);
        }
        catch {
            return undefined;
        }
        const files = names.filter(name => name.endsWith('.json')).map(name => join(dir, name))
            .filter(path => path !== this.file).sort((a, b) => {
            try {
                return statSync(b).mtimeMs - statSync(a).mtimeMs;
            }
            catch {
                return 0;
            }
        });
        for (const path of files) {
            try {
                if (readPrivateJson(path)['cwd'] === this.cwd)
                    return path;
            }
            catch { /* skip damaged checkpoints */ }
        }
        return undefined;
    }
}
export function readPrivateJson(path) {
    const stat = lstatSync(path);
    if (!stat.isFile() || stat.size > 32 * 1024 * 1024)
        throw new Error('Checkpoint must be a regular JSON file of at most 32 MiB');
    const value = JSON.parse(readFileSync(path, 'utf8'));
    if (!value || typeof value !== 'object' || Array.isArray(value))
        throw new Error('Invalid checkpoint');
    return value;
}
export function restoreFileCheckpoint(checkpoint, ctx) {
    const saved = readPrivateJson(checkpoint);
    const path = saved['path'];
    if (saved['version'] !== 1 || typeof path !== 'string' || !isAbsolute(path) || typeof saved['afterHash'] !== 'string' ||
        (saved['before'] !== null && (typeof saved['before'] !== 'string' || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(saved['before']))))
        throw new Error('Invalid file checkpoint');
    if (!lstatSync(path).isFile())
        throw new Error('Checkpoint target is no longer a regular file');
    const current = readFileSync(path);
    if (digest(current) !== saved['afterHash'])
        throw new Error('File changed after this checkpoint; refusing to overwrite newer work');
    ctx.signal.throwIfAborted();
    if (saved['before'] === null) {
        unlinkSync(path);
        syncDirectory(dirname(path));
        return `Removed checkpoint-created file ${path}`;
    }
    const next = replaceProjectFile(path, Buffer.from(saved['before'], 'base64'), ctx, { expected: current });
    return `Restored ${path}\nRecovery checkpoint: ${next}`;
}
