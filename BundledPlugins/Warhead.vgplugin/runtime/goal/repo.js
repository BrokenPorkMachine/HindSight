/**
 * Repository observation for the goal supervisor.
 *
 * Everything here is read-only and bounded. The supervisor needs a cheap,
 * deterministic view of "what state was this task planned against" and "did the
 * last attempt actually change anything", not a second copy of git.
 */
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { closeSync, existsSync, openSync, readFileSync, readSync, readdirSync, statSync } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';
const MAX_STATUS_BYTES = 64 * 1024;
export function hashBytes(value) {
    return createHash('sha256').update(value).digest('hex');
}
export function hashFile(path) {
    try {
        if (!statSync(path).isFile())
            return null;
        // Hash in fixed-size chunks instead of allocating the entire file.
        const fd = openSync(path, 'r');
        try {
            const hash = createHash('sha256');
            const buffer = Buffer.allocUnsafe(64 * 1024);
            let bytes = 0;
            while ((bytes = readSync(fd, buffer, 0, buffer.length, null)) > 0)
                hash.update(buffer.subarray(0, bytes));
            return hash.digest('hex');
        }
        finally {
            closeSync(fd);
        }
    }
    catch {
        return null;
    }
}
function git(workspace, args) {
    try {
        const result = spawnSync('git', args, {
            cwd: workspace, encoding: 'utf8', timeout: 10_000,
            maxBuffer: MAX_STATUS_BYTES, stdio: ['ignore', 'pipe', 'pipe'],
        });
        return { ok: result.status === 0, text: result.status === 0 ? (result.stdout ?? '').trim() : (result.stderr ?? '').trim() };
    }
    catch {
        return { ok: false, text: '' };
    }
}
/** State against which a plan was made. The fingerprint deliberately includes
 *  content hashes of the changed files, not only `git status`, because an edit
 *  that keeps the same path dirty is real progress. */
export function repoSnapshot(workspace) {
    const inside = git(workspace, ['rev-parse', '--is-inside-work-tree']);
    if (!inside.ok || inside.text !== 'true') {
        return { isRepo: false, branch: null, commit: null, dirty: false, statusText: '', fingerprint: 'not-a-repository' };
    }
    const branch = git(workspace, ['rev-parse', '--abbrev-ref', 'HEAD']).text || null;
    const commit = git(workspace, ['rev-parse', 'HEAD']).text || null;
    const statusText = git(workspace, ['status', '--porcelain=v1', '--untracked-files=normal']).text;
    const parts = [branch ?? '', commit ?? '', statusText];
    for (const path of changedPaths(statusText)) {
        parts.push(`${path}:${hashFile(resolve(workspace, path)) ?? 'missing'}`);
    }
    return {
        isRepo: true, branch, commit, dirty: statusText !== '',
        statusText, fingerprint: hashBytes(parts.join('\n')),
    };
}
/** Paths from `git status --porcelain=v1`, including renames. */
export function changedPaths(statusText) {
    const out = [];
    for (const line of statusText.split('\n')) {
        if (line.length < 4)
            continue;
        let path = line.slice(3).trim();
        if (path.includes(' -> '))
            path = path.split(' -> ').pop().trim();
        if (path.startsWith('"') && path.endsWith('"'))
            path = path.slice(1, -1);
        if (path !== '')
            out.push(path);
    }
    return [...new Set(out)].slice(0, 200);
}
/** A bounded inventory for the planner. Listing every file in a large repo is
 *  neither affordable nor useful; two directory levels plus manifests is enough
 *  to choose a sensible first task. */
export function workspaceOverview(workspace, limit = 120) {
    const files = [];
    const manifests = [];
    const walk = (dir, depth) => {
        if (files.length >= limit || depth > 2)
            return;
        let entries;
        try {
            entries = readdirSync(dir, { withFileTypes: true });
        }
        catch {
            return;
        }
        for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
            if (files.length >= limit)
                return;
            if (entry.name === 'node_modules' || entry.name === '.git' || entry.name === 'dist' || entry.name === '.build' || entry.name === 'target')
                continue;
            const full = join(dir, entry.name);
            const rel = relative(workspace, full);
            if (entry.isDirectory())
                walk(full, depth + 1);
            else if (entry.isFile()) {
                files.push(rel);
                if (/^(?:package\.json|tsconfig[^/]*\.json|Cargo\.toml|go\.mod|pyproject\.toml|Makefile|README\.md)$/.test(entry.name))
                    manifests.push(rel);
            }
        }
    };
    walk(workspace, 0);
    const scripts = [];
    const pkg = join(workspace, 'package.json');
    if (existsSync(pkg)) {
        try {
            const parsed = JSON.parse(readFileSync(pkg, 'utf8'));
            for (const [name, command] of Object.entries(parsed.scripts ?? {}).slice(0, 40)) {
                scripts.push(`${name}: ${command}`);
            }
        }
        catch { /* malformed manifest is not fatal to planning */ }
    }
    const note = files.length >= limit ? `inventory truncated at ${limit} files` : `${files.length} file(s) observed`;
    return { files, scripts, manifests, note };
}
/** Conservative, language-aware acceptance commands. Anything discovered here
 *  still passes through the permission gate before it runs. */
export function detectVerificationCommands(workspace) {
    const hints = [];
    const pkg = join(workspace, 'package.json');
    if (existsSync(pkg)) {
        try {
            const parsed = JSON.parse(readFileSync(pkg, 'utf8'));
            const scripts = parsed.scripts ?? {};
            for (const name of ['typecheck', 'build', 'test', 'lint']) {
                if (typeof scripts[name] === 'string') {
                    hints.push({ description: `npm run ${name} succeeds`, command: name === 'typecheck' ? 'npm run typecheck' : `npm run ${name}` });
                }
            }
        }
        catch { /* ignore */ }
    }
    if (!hints.some(hint => hint.command.includes('test'))) {
        if (existsSync(join(workspace, 'Cargo.toml'))) {
            hints.push({ description: 'cargo build succeeds', command: 'cargo build' });
            hints.push({ description: 'cargo test passes', command: 'cargo test' });
        }
        else if (existsSync(join(workspace, 'go.mod'))) {
            hints.push({ description: 'go build succeeds', command: 'go build ./...' });
            hints.push({ description: 'go test passes', command: 'go test ./...' });
        }
        else if (existsSync(join(workspace, 'pyproject.toml')) || existsSync(join(workspace, 'pytest.ini'))) {
            hints.push({ description: 'pytest passes', command: 'python3 -m pytest -q' });
        }
    }
    return hints.slice(0, 4);
}
export function resolveInside(workspace, relativeOrAbsolute) {
    const target = isAbsolute(relativeOrAbsolute) ? relativeOrAbsolute : resolve(workspace, relativeOrAbsolute);
    const rel = relative(workspace, target);
    return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel)) ? target : null;
}
