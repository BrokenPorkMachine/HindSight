// src/tools/cache.ts
// Simple in‑process cache for read‑only tool outcomes.
// This cache lives only for the current process (appropriate for the CLI/Repl).
// It stores successful outcomes (ok === true) keyed by a stable hash of the
// tool name and its JSON‑stringified arguments.
import { createHash } from 'node:crypto';
export class ToolCache {
    map = new Map();
    maxEntries = 128;
    maxValueChars = 16_384;
    key(name, args) {
        const h = createHash('sha256');
        h.update(name);
        h.update(JSON.stringify(args));
        return h.digest('hex');
    }
    get(name, args) {
        const key = this.key(name, args);
        const entry = this.map.get(key);
        if (entry) {
            this.map.delete(key);
            this.map.set(key, entry);
        }
        return entry;
    }
    set(name, args, outcome) {
        if (outcome.ok && outcome.output.length <= this.maxValueChars) {
            const key = this.key(name, args);
            this.map.delete(key);
            this.map.set(key, outcome);
            while (this.map.size > this.maxEntries)
                this.map.delete(this.map.keys().next().value);
        }
    }
    clear() {
        this.map.clear();
    }
}
// Global singleton used by the tool registry.
export const toolCache = new ToolCache();
