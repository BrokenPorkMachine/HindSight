/** Bounded single-file compression and decompression. No shell interpolation.
 * zlib formats use Node streams; bzip2 is an optional external executable. */
import { createReadStream, createWriteStream, statSync, existsSync } from 'node:fs';
import { pipeline } from 'node:stream/promises';
import { Transform } from 'node:stream';
import { spawn } from 'node:child_process';
import { createGzip, createGunzip, createDeflate, createInflate, createBrotliCompress, createBrotliDecompress } from 'node:zlib';
import { resolve, dirname, basename, join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { reqString, ArgError } from './args.js';
const MAX_INPUT = 512 * 1024 * 1024;
const MAX_OUTPUT = 1024 * 1024 * 1024;
const codecs = new Set(['gzip', 'deflate', 'brotli', 'bzip2']);
export const compressFile = {
    mutates: true,
    def: {
        name: 'compress_file',
        description: 'Compress or decompress one file using gzip, deflate, brotli (Node zlib) or optional bzip2. No archive unpacking or shell evaluation; output must not exist.',
        parameters: { type: 'object', properties: {
                input: { type: 'string', description: 'Source file path' },
                output: { type: 'string', description: 'New destination path; must not exist' },
                format: { type: 'string', enum: ['gzip', 'deflate', 'brotli', 'bzip2'] },
                operation: { type: 'string', enum: ['compress', 'decompress'] },
            }, required: ['input', 'output', 'format', 'operation'] },
    },
    async run(args, ctx) {
        let input, output, format, operation;
        try {
            input = resolve(ctx.cwd, reqString(args, 'input'));
            output = resolve(ctx.cwd, reqString(args, 'output'));
            format = reqString(args, 'format');
            operation = reqString(args, 'operation');
        }
        catch (error) {
            return { ok: false, output: '', error: error instanceof ArgError ? error.message : String(error) };
        }
        if (!codecs.has(format) || !['compress', 'decompress'].includes(operation))
            return { ok: false, output: '', error: 'unsupported format or operation' };
        if (input === output || existsSync(output))
            return { ok: false, output: '', error: 'output already exists or equals input' };
        try {
            const size = statSync(input);
            if (!size.isFile() || size.size > MAX_INPUT)
                return { ok: false, output: '', error: 'input is not a regular file or exceeds 512 MiB' };
        }
        catch (e) {
            return { ok: false, output: '', error: String(e) };
        }
        const signal = ctx.signal;
        if (signal.aborted)
            return { ok: false, output: '', error: 'cancelled' };
        const { unlink, link } = await import('node:fs/promises');
        const { workspacePathAllowed } = await import('./registry.js');
        if (ctx.workspaceRoot && (!workspacePathAllowed(ctx.workspaceRoot, ctx.cwd, input) || !workspacePathAllowed(ctx.workspaceRoot, ctx.cwd, output)))
            return { ok: false, output: '', error: 'workspace scope denied' };
        // Publish via an exclusive hard link: rename() could overwrite an output
        // created by another process while compression was running.
        const temporary = join(dirname(output), `.${basename(output)}.${randomUUID()}.partial`);
        let written = 0;
        const meter = new Transform({
            transform(chunk, _encoding, done) {
                written += chunk.length;
                done(written > MAX_OUTPUT ? new Error('decompressed output exceeds 1 GiB') : null, chunk);
            },
        });
        try {
            if (format === 'bzip2') {
                const child = spawn('bzip2', [operation === 'compress' ? '-c' : '-dc', '--', input], { stdio: ['ignore', 'pipe', 'pipe'] });
                let stderr = '';
                child.stderr.on('data', (b) => { stderr = (stderr + b.toString()).slice(-2000); });
                const exited = new Promise((ok, fail) => {
                    child.once('error', fail);
                    child.once('close', code => code === 0 ? ok() : fail(new Error(stderr || `bzip2 exited ${code}`)));
                });
                const abort = () => child.kill('SIGKILL');
                signal.addEventListener('abort', abort, { once: true });
                try {
                    await Promise.all([pipeline(child.stdout, meter, createWriteStream(temporary, { flags: 'wx', mode: 0o600 }), { signal }), exited]);
                }
                finally {
                    signal.removeEventListener('abort', abort);
                    if (child.exitCode === null)
                        child.kill('SIGKILL');
                }
            }
            else {
                const codec = format === 'gzip' ? (operation === 'compress' ? createGzip() : createGunzip())
                    : format === 'deflate' ? (operation === 'compress' ? createDeflate() : createInflate())
                        : operation === 'compress' ? createBrotliCompress() : createBrotliDecompress();
                // Every codec must write to the private staging path. Publishing
                // directly to output both exposed partial data and left nothing for
                // link(temporary, output), causing ENOENT on successful gzip runs.
                await pipeline(createReadStream(input), codec, meter, createWriteStream(temporary, { flags: 'wx', mode: 0o600 }), { signal });
            }
            await link(temporary, output); // Fails with EEXIST without modifying the existing file.
            return { ok: true, output: `${operation} ${format}: ${input} -> ${output} (${written} bytes)` };
        }
        catch (error) {
            // Only remove the private temporary file we created; never unlink output.
            await unlink(temporary).catch(() => { });
            return { ok: false, output: '', error: error instanceof Error ? error.message : String(error) };
        }
        finally {
            await unlink(temporary).catch(() => { });
        }
    },
};
