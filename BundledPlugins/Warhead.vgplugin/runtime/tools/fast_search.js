/** Optional ripgrep-backed repository search. No shell, bounded output and runtime. */
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';
import { reqString, optString, optInt, optBool, ArgError } from './args.js';
export const fastSearch = {
    mutates: false,
    def: {
        name: 'fast_search',
        description: 'Fast workspace text search via ripgrep (rg). Honors .gitignore, skips binary files, returns file:line:text. Requires rg on PATH; use grep if unavailable.',
        parameters: {
            type: 'object',
            properties: {
                pattern: { type: 'string', description: 'Regex pattern (ripgrep syntax)' },
                path: { type: 'string', description: 'Workspace subdirectory or file (default .)' },
                include: { type: 'string', description: 'Glob of files to include, e.g. *.ts' },
                ignoreCase: { type: 'boolean' },
                limit: { type: 'integer', description: 'Maximum lines, 1-500 (default 100)' },
            },
            required: ['pattern'],
        },
    },
    async run(args, ctx) {
        let pattern, path, include, ignoreCase, limit;
        try {
            pattern = reqString(args, 'pattern');
            path = optString(args, 'path') ?? '.';
            include = optString(args, 'include');
            ignoreCase = optBool(args, 'ignoreCase') ?? false;
            limit = Math.min(500, optInt(args, 'limit', 1) ?? 100);
        }
        catch (e) {
            return { ok: false, output: '', error: e instanceof ArgError ? e.message : String(e) };
        }
        if (ctx.signal.aborted)
            return { ok: false, output: '', error: 'cancelled' };
        const argv = ['--line-number', '--with-filename', '--no-heading', '--color=never', '--max-columns=350', '--max-columns-preview', '--max-count', String(limit), '--glob', '!.git/**'];
        if (ignoreCase)
            argv.push('--ignore-case');
        if (include)
            argv.push('--glob', include);
        argv.push('--', pattern, resolve(ctx.cwd, path));
        return await new Promise(finish => {
            const child = spawn('rg', argv, { cwd: ctx.cwd, stdio: ['ignore', 'pipe', 'pipe'] });
            let output = '', stderr = '', lines = 0, settled = false, timedOut = false, limited = false, pending = '';
            const timeout = Math.min(60_000, ctx.config().limits.toolTimeoutMs ?? 30_000);
            const timer = setTimeout(() => { timedOut = true; child.kill('SIGKILL'); }, timeout);
            const abort = () => child.kill('SIGKILL');
            const complete = (result) => {
                if (settled)
                    return;
                settled = true;
                clearTimeout(timer);
                ctx.signal.removeEventListener('abort', abort);
                finish(result);
            };
            ctx.signal.addEventListener('abort', abort, { once: true });
            child.stdout.on('data', (chunk) => {
                if (limited)
                    return;
                pending += chunk.toString('utf8');
                let newline;
                while ((newline = pending.indexOf('\n')) !== -1 && lines < limit && output.length < 64_000) {
                    const line = pending.slice(0, newline + 1);
                    pending = pending.slice(newline + 1);
                    output += line.slice(0, Math.max(0, 64_000 - output.length));
                    lines++;
                }
                if (lines >= limit || output.length >= 64_000 || pending.length > 64_000) {
                    limited = true;
                    child.kill('SIGTERM');
                }
            });
            child.stderr.on('data', (chunk) => { stderr += chunk.toString('utf8').slice(0, Math.max(0, 3000 - stderr.length)); });
            child.on('error', e => complete({ ok: false, output: '', error: e.code === 'ENOENT' ? 'ripgrep (rg) is not installed; use grep or install rg' : e.message }));
            child.on('close', code => {
                if (ctx.signal.aborted)
                    return complete({ ok: false, output: '', error: 'cancelled' });
                if (timedOut)
                    return complete({ ok: false, output, error: 'ripgrep timed out' });
                if (code !== 0 && code !== 1 && !limited)
                    return complete({ ok: false, output, error: stderr || `rg exited ${code}` });
                complete({ ok: true, output: (output + (!limited && pending ? pending.slice(0, Math.max(0, 64_000 - output.length)) : '') || 'No matches.') + (limited ? '\n[search output limit reached]' : '') });
            });
            if (ctx.signal.aborted)
                abort();
        });
    },
};
