/**
 * edit_file — exact string replacement.
 *
 * Requires `old_string` to be unique unless replace_all is set. Silently
 * replacing the first of several matches is how an edit lands in the wrong
 * place, so ambiguity is reported as an error rather than resolved by guessing.
 *
 * Matching is done on the raw file text, not on a line-numbered rendering, so
 * the model can paste back exactly what read_file showed it (minus the line
 * number gutter).
 */
import { readFileSync, statSync } from 'node:fs';
import { replaceProjectFile } from './files.js';
import { resolve, isAbsolute } from 'node:path';
import { reqString, ArgError } from './args.js';
import { coerceEdit, EDIT_SHAPE_HELP, describeKeys } from './edits.js';
export const MAX_EDIT_BYTES = 5 * 1024 * 1024;
function countOccurrences(haystack, needle) {
    if (needle === '')
        return 0;
    let count = 0;
    let idx = haystack.indexOf(needle);
    while (idx !== -1) {
        count++;
        idx = haystack.indexOf(needle, idx + needle.length);
    }
    return count;
}
/** Show a little context around each match so an ambiguous edit is diagnosable
 *  from the error message alone. */
function context(text, needle, occurrences) {
    const parts = [];
    let idx = text.indexOf(needle);
    let shown = 0;
    while (idx !== -1 && shown < occurrences) {
        const from = Math.max(0, idx - 40);
        const to = Math.min(text.length, idx + needle.length + 40);
        const line = text.slice(0, idx).split('\n').length;
        parts.push(`  line ${line}: …${text.slice(from, to).replace(/\n/g, '\\n')}…`);
        idx = text.indexOf(needle, idx + needle.length);
        shown++;
    }
    return parts.join('\n');
}
/** Explain WHY a match failed, using the most likely cause.
 *
 *  The dominant observed cause is the model copying read_file's line-number
 *  gutter into old_string ("47\t  bashTimeoutMs: number" instead of
 *  "  bashTimeoutMs: number"). Showing the nearest real line makes that mistake
 *  self-evident in the error, which is far more effective than restating
 *  "not found". */
function diagnoseMiss(text, oldStr) {
    const fileLines = text.split('\n');
    // Case 1: every non-empty fragment appears, but only when the gutter is stripped.
    const gutterStripped = oldStr
        .split('\n')
        .map(l => l.replace(/^\s*\d+\t/, ''))
        .join('\n');
    if (gutterStripped !== oldStr && text.includes(gutterStripped)) {
        return (` Your old_string matched after removing read_file's line-number gutter, so it still contains ` +
            `the "NNN\\t" prefix. Strip the line numbers and use only the file's own text.`);
    }
    // Case 2: report the closest line so the real text is visible.
    const needle = oldStr.split('\n').map(l => l.trim()).filter(l => l.length > 3)[0];
    if (needle) {
        let best = -1;
        let bestScore = 0;
        for (let i = 0; i < fileLines.length; i++) {
            const score = similarity(fileLines[i].trim(), needle);
            if (score > bestScore) {
                bestScore = score;
                best = i;
            }
        }
        if (best >= 0 && bestScore > 0.4) {
            return (` Closest line in the file is line ${best + 1}: ${JSON.stringify(fileLines[best])}` +
                ` — copy the file's text exactly, without the line-number gutter.`);
        }
    }
    return ' Re-read the file to get the current text; it may have changed.';
}
/** Cheap token-overlap ratio. Good enough to suggest a nearby line. */
function similarity(a, b) {
    if (a === '' || b === '')
        return 0;
    if (a === b)
        return 1;
    const at = new Set(a.split(/\s+/));
    const bt = new Set(b.split(/\s+/));
    let shared = 0;
    for (const t of bt)
        if (at.has(t))
            shared++;
    return shared / Math.max(at.size, bt.size);
}
export const editFile = {
    mutates: true,
    def: {
        name: 'edit',
        description: 'Replace an exact string in a file. old_string must appear exactly once ' +
            'unless replace_all is true. Copy old_string verbatim from what read_file ' +
            'returned (without the line-number gutter). Read the file first.',
        parameters: {
            type: 'object',
            properties: {
                path: { type: 'string', description: 'Path to the file' },
                old_string: { type: 'string', description: 'Exact text to find' },
                new_string: { type: 'string', description: 'Replacement text' },
                replace_all: {
                    type: 'boolean',
                    description: 'Replace every occurrence instead of requiring uniqueness.',
                },
            },
            required: ['path', 'old_string', 'new_string'],
        },
    },
    async run(args, ctx) {
        let path;
        let oldStr;
        let newStr;
        let replaceAll;
        try {
            path = reqString(args, 'path');
            const c = coerceEdit(args);
            if (!c) {
                throw new ArgError(`missing old/new string pair — ${EDIT_SHAPE_HELP.replace('{keys}', describeKeys(args))}`);
            }
            oldStr = c.oldString;
            newStr = c.newString;
            replaceAll = c.replaceAll;
        }
        catch (err) {
            return { ok: false, output: '', error: err instanceof ArgError ? err.message : String(err) };
        }
        if (oldStr === newStr) {
            return { ok: false, output: '', error: 'old_string and new_string are identical; nothing to do.' };
        }
        if (oldStr === '') {
            return {
                ok: false,
                output: '',
                error: 'old_string is empty; use the write tool to create a file.',
            };
        }
        const abs = isAbsolute(path) ? path : resolve(ctx.cwd, path);
        let st;
        try {
            st = statSync(abs);
            if (st.isDirectory())
                return { ok: false, output: '', error: `${abs} is a directory` };
        }
        catch {
            return { ok: false, output: '', error: `File not found: ${abs}. Use the write tool to create it.` };
        }
        if (st.size > MAX_EDIT_BYTES) {
            return { ok: false, output: '', error: `${abs} is ${st.size} bytes; too large to edit in place.` };
        }
        let text;
        try {
            text = readFileSync(abs, 'utf8');
        }
        catch (err) {
            return { ok: false, output: '', error: `Cannot read ${abs}: ${err.message}` };
        }
        const occurrences = countOccurrences(text, oldStr);
        if (occurrences === 0) {
            const collapsedOld = oldStr.replace(/\s+/g, ' ');
            const collapsedHit = collapsedOld.length > 12 && text.replace(/\s+/g, ' ').includes(collapsedOld);
            return {
                ok: false,
                output: '',
                error: `old_string not found in ${abs}.` +
                    (collapsedHit
                        ? ' It matches once whitespace is collapsed, so the indentation or line breaks differ — read the exact bytes again.'
                        : '') +
                    diagnoseMiss(text, oldStr),
            };
        }
        if (occurrences > 1 && !replaceAll) {
            return {
                ok: false,
                output: '',
                error: `old_string is not unique in ${abs} — it appears ${occurrences} times. ` +
                    `Include more surrounding context to make it unique, or set replace_all: true.\n` +
                    context(text, oldStr, occurrences),
            };
        }
        const updated = replaceAll
            ? text.split(oldStr).join(newStr)
            : text.slice(0, text.indexOf(oldStr)) + newStr + text.slice(text.indexOf(oldStr) + oldStr.length);
        const bytes = Buffer.byteLength(updated, 'utf8');
        if (bytes > MAX_EDIT_BYTES) {
            return { ok: false, output: '', error: `Edit result is ${bytes} bytes; limit is ${MAX_EDIT_BYTES}.` };
        }
        let checkpoint;
        try {
            checkpoint = replaceProjectFile(abs, updated, ctx, { expected: text });
        }
        catch (err) {
            return { ok: false, output: '', error: `Cannot write ${abs}: ${err.message}` };
        }
        const n = occurrences === 0 ? 0 : replaceAll ? occurrences : 1;
        return {
            ok: true,
            output: `Edited ${abs} — replaced ${n} occurrence(s).\nRecovery checkpoint: ${checkpoint}`,
        };
    },
};
