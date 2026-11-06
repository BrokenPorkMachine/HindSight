import { lstatSync, mkdirSync, readdirSync, unlinkSync } from 'node:fs';
import { join } from 'node:path';
/** Retain bounded private snapshots, keeping the current checkpoint. */
export function pruneRecovery(dir, keepPath) {
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    const entries = readdirSync(dir).filter(name => name.endsWith('.json')).flatMap(name => {
        const path = join(dir, name);
        try {
            const st = lstatSync(path);
            return st.isFile() ? [{ path, size: st.size, time: st.mtimeMs }] : [];
        }
        catch {
            return [];
        }
    }).sort((a, b) => b.time - a.time);
    let retained = 0, bytes = 0;
    for (const entry of entries) {
        if (entry.path === keepPath || (retained < 100 && bytes + entry.size <= 128 * 1024 * 1024)) {
            retained++;
            bytes += entry.size;
            continue;
        }
        try {
            unlinkSync(entry.path);
        }
        catch { /* another process may be pruning */ }
    }
}
