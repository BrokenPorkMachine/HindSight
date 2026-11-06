/**
 * grep — search file contents by regular expression.
 *
 * Deliberately not trying to be ripgrep. One pattern, one optional include
 * filter, `file:line:content` output. The agentic value is in the model
 * choosing good patterns, not in search-engine features.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { resolve, isAbsolute, join, basename } from 'node:path';
import { reqString, optString, optInt, optBool, ArgError } from './args.js';
import { globToRegExp } from './glob.js';
import { isMainThread } from 'node:worker_threads';
import { runSearch } from './search.js';
const DEFAULT_LIMIT = 100;
const ALWAYS_SKIP = new Set(['.git', 'node_modules', '.DS_Store', 'dist', 'build', '.next']);
const MAX_FILE_BYTES = 2 * 1024 * 1024;
const MAX_LINE_CHARS = 400;
function walk(root, depth, acc) {
    if (depth > 12 || acc.length >= 50_000)
        return;
    let entries;
    try {
        entries = readdirSync(root, { withFileTypes: true });
    }
    catch {
        return;
    }
    for (const e of entries) {
        if (acc.length >= 50_000)
            break;
        if (e.name.startsWith('.') && e.name !== '.claude')
            continue;
        if (ALWAYS_SKIP.has(e.name))
            continue;
        const full = join(root, e.name);
        if (e.isDirectory())
            walk(full, depth + 1, acc);
        else if (e.isFile())
            acc.push(full);
    }
}
export const grep = {
    mutates: false,
    def: {
        name: 'grep',
        description: 'Search file contents with a regular expression. Returns matches as ' +
            'file:line:content. Use include to restrict to certain file types, e.g. ' +
            'include="*.ts". Prefer this over shelling out to grep.',
        parameters: {
            type: 'object',
            properties: {
                pattern: { type: 'string', description: 'JavaScript regular expression to search for' },
                path: { type: 'string', description: 'File or directory to search. Defaults to working directory.' },
                include: { type: 'string', description: 'Only search files matching this glob, e.g. "*.ts"' },
                limit: { type: 'integer', description: `Maximum matches. Defaults to ${DEFAULT_LIMIT}.` },
                ignoreCase: { type: 'boolean', description: 'Case-insensitive match' },
            },
            required: ['pattern'],
        },
    },
    async run(args, ctx) {
        if (isMainThread)
            return runSearch('grep', args, ctx);
        let pattern;
        let target;
        let include;
        let limit;
        let ignoreCase;
        try {
            pattern = reqString(args, 'pattern');
            target = optString(args, 'path');
            include = optString(args, 'include');
            limit = Math.min(1000, optInt(args, 'limit', 1) ?? DEFAULT_LIMIT);
            ignoreCase = optBool(args, 'ignoreCase') ?? false;
        }
        catch (err) {
            return { ok: false, output: '', error: err instanceof ArgError ? err.message : String(err) };
        }
        let re;
        try {
            re = new RegExp(pattern, ignoreCase ? 'i' : '');
        }
        catch (err) {
            return { ok: false, output: '', error: `Invalid regular expression: ${err.message}` };
        }
        const base = isAbsolute(target ?? '') ? target : resolve(ctx.cwd, target ?? '.');
        let files;
        try {
            const st = statSync(base);
            if (st.isDirectory()) {
                files = [];
                walk(base, 0, files);
            }
            else {
                files = [base];
            }
        }
        catch {
            return { ok: false, output: '', error: `Path not found: ${base}` };
        }
        // Include semantics follow ripgrep: a pattern containing a slash is matched
        // against the path relative to the search root, otherwise against the
        // basename. Testing the raw absolute path instead would make every
        // rooted include like "src/config.ts" silently match nothing.
        const includeHasSlash = include ? include.includes('/') : false;
        const includeRe = include
            ? globToRegExp(includeHasSlash ? include : `**/${include}`)
            : null;
        const hits = [];
        let filesWithHits = 0;
        let truncated = false;
        for (const f of files) {
            if (hits.length >= limit) {
                truncated = true;
                break;
            }
            if (includeRe) {
                const relToBase = f.startsWith(base + '/') ? f.slice(base.length + 1) : f;
                const subject = includeHasSlash ? relToBase : basename(f);
                if (!includeRe.test(subject))
                    continue;
            }
            let st;
            try {
                st = statSync(f);
            }
            catch {
                continue;
            }
            if (st.size > MAX_FILE_BYTES)
                continue;
            let text;
            try {
                const buf = readFileSync(f);
                if (buf.subarray(0, 4096).includes(0))
                    continue; // binary
                text = buf.toString('utf8');
            }
            catch {
                continue;
            }
            const lines = text.split('\n');
            let fileHadHit = false;
            for (let i = 0; i < lines.length; i++) {
                const line = lines[i];
                if (!re.test(line))
                    continue;
                if (hits.length >= limit) {
                    truncated = true;
                    break;
                }
                fileHadHit = true;
                const shown = line.length > MAX_LINE_CHARS ? line.slice(0, MAX_LINE_CHARS) + ' …' : line;
                hits.push(`${f}:${i + 1}:${shown.trim()}`);
            }
            if (fileHadHit)
                filesWithHits++;
        }
        if (hits.length === 0) {
            return { ok: true, output: `No matches for /${pattern}/ under ${base}.` };
        }
        const tail = (truncated ? `\n[stopped at ${limit} matches; narrow the pattern or path]` : '') +
            (files.length >= 50_000 ? '\n[search limited to 50000 files; narrow the path]' : '');
        return {
            ok: true,
            output: `${hits.length} match(es) in ${filesWithHitLabel(filesWithHits)}:\n${hits.join('\n')}${tail}`,
        };
    },
};
function filesWithHitLabel(n) {
    return n === 1 ? '1 file' : `${n} files`;
}
