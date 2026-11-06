/**
 * read_file — read a file with an optional line window.
 *
 * Output is `cat -n` style with right-aligned line numbers. That convention is
 * load-bearing rather than cosmetic: the model is told to cite `file:line`, and
 * padded numbers make the cited line findable by eye when the model later says
 * "see line 412" instead of restating the code.
 *
 * The file is STREAMED, never slurped. Reading it whole to show five lines cost
 * ~1GB of transient allocation on a 400MB input (Buffer plus its UTF-16 string)
 * for 2KB of output. Windowed streaming bounds memory regardless of file size
 * and lets a binary file be detected from its first chunk instead of after the
 * entire download.
 */
import { createReadStream } from 'node:fs';
import { statSync } from 'node:fs';
import { resolve, isAbsolute } from 'node:path';
import { reqString, optInt, ArgError } from './args.js';
const DEFAULT_LIMIT = 2000;
const MAX_LINE_CHARS = 2000;
const CHUNK_BYTES = 64 * 1024;
/** Above this, an exact total-line count is not worth a second full pass. */
const COUNT_EXACTLY_BELOW_BYTES = 8 * 1024 * 1024;
function streamWindow(abs, offset, limit, wantTotal) {
    return new Promise((resolvePromise, rejectPromise) => {
        const lines = [];
        let buf = '';
        let lineNo = 0;
        let scannedTo = 0;
        let binary = false;
        let settled = false;
        /** True while discarding the remainder of an over-long line. */
        let skippingLongLine = false;
        const stream = createReadStream(abs, { encoding: 'utf8', highWaterMark: CHUNK_BYTES });
        const finish = (reachedEnd) => {
            if (settled)
                return;
            settled = true;
            stream.destroy();
            resolvePromise({
                lines,
                scannedTo,
                reachedEnd,
                binary,
                totalLines: wantTotal && reachedEnd ? scannedTo : null,
            });
        };
        // Consume a chunk, appending complete lines. `buf` only ever holds the
        // trailing partial line, so each chunk costs O(chunk).
        //
        // The over-long-line path matters more than it looks: a minified bundle or a
        // 400MB single-line file made the first version quadratic, because while
        // "skipping" a long line the buffer was still accumulated and rescanned.
        const consume = (text) => {
            buf += text;
            let s = 0;
            let nl;
            while ((nl = buf.indexOf('\n', s)) !== -1) {
                const raw = buf.slice(s, nl);
                s = nl + 1;
                lineNo++;
                const clipped = raw.length > MAX_LINE_CHARS ? raw.slice(0, MAX_LINE_CHARS) + ' …[truncated]' : raw;
                if (lineNo >= offset && lines.length < limit)
                    lines.push(clipped);
                scannedTo = lineNo;
                if (lines.length >= limit)
                    return true;
            }
            buf = s > 0 ? buf.slice(s) : buf;
            if (buf.length > MAX_LINE_CHARS) {
                // Emit the visible prefix, then discard until the next newline.
                lineNo++;
                const clipped = buf.slice(0, MAX_LINE_CHARS) + ' …[truncated, line continues]';
                if (lineNo >= offset && lines.length < limit)
                    lines.push(clipped);
                scannedTo = lineNo;
                buf = '';
                skippingLongLine = true;
                return lines.length >= limit;
            }
            return false;
        };
        stream.on('data', (chunk) => {
            if (settled)
                return;
            const text = typeof chunk === 'string' ? chunk : chunk.toString('utf8');
            if (!binary && text.includes('\u0000')) {
                binary = true;
                finish(false);
                return;
            }
            if (skippingLongLine) {
                // A newline is never split across chunks once the stream is decoding to
                // strings, so a single indexOf decides whether this chunk is entirely
                // discard or begins the next line.
                const nl = text.indexOf('\n');
                if (nl === -1)
                    return;
                skippingLongLine = false;
                if (consume(text.slice(nl + 1)))
                    finish(false);
                return;
            }
            if (consume(text))
                finish(false);
        });
        stream.on('end', () => {
            // A trailing fragment with no newline is still a line.
            if (!settled && buf.length > 0 && !skippingLongLine) {
                lineNo++;
                const text = buf.length > MAX_LINE_CHARS ? buf.slice(0, MAX_LINE_CHARS) + ' …[truncated]' : buf;
                if (lineNo >= offset && lines.length < limit)
                    lines.push(text);
                scannedTo = lineNo;
            }
            finish(true);
        });
        stream.on('error', err => {
            if (settled)
                return;
            settled = true;
            rejectPromise(err);
        });
    });
}
export const readFile = {
    mutates: false,
    def: {
        name: 'read_file',
        description: 'Read a file from the local filesystem. Returns the contents with line numbers ' +
            'prefixed to each line. Use offset and limit to page through large files. ' +
            'Only use absolute paths, or paths relative to the working directory.',
        parameters: {
            type: 'object',
            properties: {
                path: { type: 'string', description: 'Path to the file to read' },
                offset: {
                    type: 'integer',
                    description: 'First line to read, 1-indexed. Omit to start at the beginning.',
                },
                limit: {
                    type: 'integer',
                    description: `Maximum number of lines to read. Defaults to ${DEFAULT_LIMIT}.`,
                },
            },
            required: ['path'],
        },
    },
    async run(args, ctx) {
        let rawPath;
        let offset;
        let limit;
        try {
            rawPath = reqString(args, 'path');
            offset = optInt(args, 'offset', 1);
            limit = optInt(args, 'limit', 1);
        }
        catch (err) {
            return { ok: false, output: '', error: err instanceof ArgError ? err.message : String(err) };
        }
        const abs = isAbsolute(rawPath) ? rawPath : resolve(ctx.cwd, rawPath);
        let st;
        try {
            st = statSync(abs);
        }
        catch {
            return { ok: false, output: '', error: `File not found: ${abs}` };
        }
        if (st.isDirectory()) {
            return { ok: false, output: '', error: `${abs} is a directory, not a file.` };
        }
        const start = (offset ?? 1) - 1;
        const want = limit ?? DEFAULT_LIMIT;
        let w;
        try {
            w = await streamWindow(abs, start + 1, want, st.size < COUNT_EXACTLY_BELOW_BYTES);
        }
        catch (err) {
            return { ok: false, output: '', error: `Cannot read ${abs}: ${err instanceof Error ? err.message : String(err)}` };
        }
        if (w.binary) {
            return {
                ok: false,
                output: '',
                error: `${abs} appears to be a binary file (${st.size} bytes); not showing contents.`,
            };
        }
        if (w.lines.length === 0) {
            if (w.totalLines !== null) {
                return {
                    ok: true,
                    output: `${abs} has ${w.totalLines} line(s). Offset ${offset ?? 1} is past the end of the file.`,
                };
            }
            return { ok: true, output: `${abs} produced no output at offset ${offset ?? 1}.` };
        }
        const width = String(start + w.lines.length).length;
        const body = w.lines
            .map((line, i) => `${String(start + i + 1).padStart(width, ' ')}\t${line}`)
            .join('\n');
        const parts = [body];
        if (start > 0)
            parts.unshift(`[showing from line ${start + 1}${w.totalLines !== null ? ` of ${w.totalLines}` : ''}]`);
        if (!w.reachedEnd) {
            const known = w.totalLines !== null ? `; ${w.totalLines - (start + w.lines.length)} more line(s)` : '';
            parts.push(`[more content follows${known}; use offset=${start + w.lines.length + 1} to continue]`);
        }
        return { ok: true, output: parts.join('\n') };
    },
};
