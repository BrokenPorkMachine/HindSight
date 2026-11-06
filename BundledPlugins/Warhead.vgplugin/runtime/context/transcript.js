/**
 * Session transcripts.
 *
 * JSONL, one event per line, so a session can be `grep`'d and `tail`'ed without
 * loading it. The transcript is for reconstructing and inspecting what the agent
 * did — not a byte-exact log — so large tool results are truncated on the way in.
 *
 * Eviction is LRU over the whole directory by total size. Per-file caps were
 * rejected because a single enormous session would then be un-evictable.
 */
import { mkdirSync, readdirSync, statSync, unlinkSync, existsSync, createWriteStream, } from 'node:fs';
import { join } from 'node:path';
import { transcriptDir } from '../config.js';
import { randomUUID } from 'node:crypto';
import { utf8Prefix } from '../text.js';
function stamp() {
    return new Date().toISOString();
}
function slug(s) {
    return s.replace(/[^a-zA-Z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 40) || 'session';
}
export class Transcript {
    stream = null;
    path = '';
    enabled;
    maxResultBytes;
    batch = [];
    batchSize = 8;
    constructor(enabled, maxResultBytes) {
        this.enabled = enabled;
        this.maxResultBytes = maxResultBytes;
    }
    get file() {
        return this.path;
    }
    start(info) {
        if (!this.enabled)
            return;
        this.close();
        try {
            const dir = transcriptDir();
            mkdirSync(dir, { recursive: true });
            const name = `${stamp().replace(/[:.]/g, '-')}_${slug(info.cwd.split('/').pop() ?? 'session')}_${randomUUID()}.jsonl`;
            this.path = join(dir, name);
            const stream = createWriteStream(this.path, { flags: 'wx', mode: 0o600 });
            this.stream = stream;
            stream.on('error', () => {
                if (this.stream === stream) {
                    this.stream = null;
                    this.batch = [];
                }
            });
            this.write({ kind: 'session_start', ts: stamp(), cwd: info.cwd, model: info.model, pid: process.pid });
        }
        catch { /* logging must not prevent startup */ }
    }
    flush() {
        if (this.batch.length === 0 || !this.stream)
            return;
        try {
            this.stream.write(this.batch.join(''));
        }
        catch { /* never let logging break the session */ }
        this.batch = [];
    }
    write(ev) {
        if (!this.stream)
            return;
        try {
            this.batch.push(JSON.stringify(ev) + '\n');
            if (this.batch.length >= this.batchSize)
                this.flush();
        }
        catch {
            /* never let logging break the session */
        }
    }
    turn(user) {
        this.write({ kind: 'turn', ts: stamp(), user });
    }
    assistant(text) {
        this.write({ kind: 'assistant', ts: stamp(), text });
    }
    toolCall(name, args) {
        this.write({ kind: 'tool_call', ts: stamp(), name, args });
    }
    toolResult(name, ok, output) {
        const prefix = utf8Prefix(output, this.maxResultBytes);
        const clipped = prefix.length < output.length
            ? `${prefix} …[truncated ${Buffer.byteLength(output) - Buffer.byteLength(prefix)} bytes]`
            : output;
        this.write({ kind: 'tool_result', ts: stamp(), name, ok, output: clipped });
    }
    stop(reason, steps) {
        this.write({ kind: 'stop', ts: stamp(), reason, steps });
        this.flush();
    }
    note(text) {
        this.write({ kind: 'note', ts: stamp(), text });
    }
    close() {
        this.flush();
        this.stream?.end();
        this.stream = null;
    }
}
/** Total bytes of all transcripts. Missing/unreadable files count as zero. */
export function transcriptBytes(dir = transcriptDir()) {
    if (!existsSync(dir))
        return 0;
    let total = 0;
    for (const f of readdirSync(dir)) {
        if (!f.endsWith('.jsonl'))
            continue;
        try {
            total += statSync(join(dir, f)).size;
        }
        catch {
            /* raced with eviction */
        }
    }
    return total;
}
/** Delete oldest transcripts until the directory fits `maxBytes`. The current
 *  session's file is never evicted — losing the log of a live session would be
 *  the wrong trade. */
export function evictTranscripts(maxBytes, keepPath) {
    const dir = transcriptDir();
    if (!existsSync(dir))
        return { removed: [], freedBytes: 0, bytesAfter: 0 };
    const entries = [];
    for (const f of readdirSync(dir)) {
        if (!f.endsWith('.jsonl'))
            continue;
        const full = join(dir, f);
        try {
            const st = statSync(full);
            entries.push({ file: full, size: st.size, mtime: st.mtimeMs });
        }
        catch {
            /* skip */
        }
    }
    entries.sort((a, b) => a.mtime - b.mtime);
    let total = entries.reduce((n, e) => n + e.size, 0);
    const removed = [];
    let freed = 0;
    for (const e of entries) {
        if (total <= maxBytes)
            break;
        if (keepPath && e.file === keepPath)
            continue;
        try {
            unlinkSync(e.file);
            removed.push(e.file);
            freed += e.size;
            total -= e.size;
        }
        catch {
            /* skip */
        }
    }
    return { removed, freedBytes: freed, bytesAfter: total };
}
export function clearTranscripts() {
    const dir = transcriptDir();
    if (!existsSync(dir))
        return 0;
    let n = 0;
    for (const f of readdirSync(dir)) {
        if (!f.endsWith('.jsonl'))
            continue;
        try {
            unlinkSync(join(dir, f));
            n++;
        }
        catch {
            /* skip */
        }
    }
    return n;
}
export function humanBytes(n) {
    if (n < 1024)
        return `${n} B`;
    if (n < 1024 * 1024)
        return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024)
        return `${(n / (1024 * 1024)).toFixed(1)} MB`;
    return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
