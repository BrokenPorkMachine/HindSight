/**
 * multi_edit — several exact replacements in one file, applied atomically.
 *
 * All edits are validated against the original text before anything is
 * written. A partial application would leave the file in a state neither the
 * model nor the user asked for, and the model would have to reconstruct it from
 * tool output. Either every edit applies or none does.
 */
import { readFileSync, statSync } from 'node:fs';
import { replaceProjectFile } from './files.js';
import { resolve, isAbsolute } from 'node:path';
import { reqString, reqArray, ArgError } from './args.js';
import { MAX_EDIT_BYTES } from './edit.js';
import { coerceEdit, EDIT_SHAPE_HELP, describeKeys } from './edits.js';
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
export const multiEdit = {
    mutates: true,
    def: {
        name: 'multi_edit',
        description: 'Apply several exact-string replacements to one file in a single atomic ' +
            'operation. Use this instead of repeated edit calls when changing multiple ' +
            'places in the same file: if any edit fails, none are applied. Edits are ' +
            'validated against the original text, so they must not overlap.',
        parameters: {
            type: 'object',
            properties: {
                path: { type: 'string', description: 'Path to the file' },
                edits: {
                    type: 'array',
                    description: 'Edits to apply, in order.',
                    items: {
                        type: 'object',
                        properties: {
                            old_string: { type: 'string', description: 'Exact text to find' },
                            new_string: { type: 'string', description: 'Replacement text' },
                            replace_all: { type: 'boolean', description: 'Replace every occurrence' },
                        },
                        required: ['old_string', 'new_string'],
                    },
                },
            },
            required: ['path', 'edits'],
        },
    },
    async run(args, ctx) {
        let path;
        let rawEdits;
        try {
            path = reqString(args, 'path');
            rawEdits = reqArray(args, 'edits');
        }
        catch (err) {
            return { ok: false, output: '', error: err instanceof ArgError ? err.message : String(err) };
        }
        const edits = [];
        const rejected = [];
        rawEdits.forEach((raw, i) => {
            const c = coerceEdit(raw);
            if (!c) {
                rejected.push(`  edit ${i + 1}: ${EDIT_SHAPE_HELP.replace('{keys}', describeKeys(raw))}`);
                return;
            }
            edits.push({ old_string: c.oldString, new_string: c.newString, replace_all: c.replaceAll });
        });
        // Atomic means atomic: if ANY entry was malformed the whole call fails.
        // Silently applying the valid subset would leave the file in a state the
        // model never described, while reporting success.
        if (rejected.length > 0) {
            return {
                ok: false,
                output: '',
                error: `No edits applied — ${rejected.length} of ${rawEdits.length} entr(y/ies) malformed.\n` +
                    rejected.join('\n'),
            };
        }
        if (edits.length === 0) {
            return { ok: false, output: '', error: 'No edits supplied.' };
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
            return { ok: false, output: '', error: `${abs} is ${st.size} bytes; too large to edit.` };
        }
        let text;
        try {
            text = readFileSync(abs, 'utf8');
        }
        catch (err) {
            return { ok: false, output: '', error: `Cannot read ${abs}: ${err.message}` };
        }
        // Validate everything first, against the original text.
        const errors = [];
        let applied = 0;
        const replacements = [];
        edits.forEach((e, i) => {
            if (e.old_string === '') {
                errors.push(`edit ${i + 1}: old_string is empty`);
                return;
            }
            if (e.old_string === e.new_string) {
                errors.push(`edit ${i + 1}: old_string and new_string are identical`);
                return;
            }
            const n = countOccurrences(text, e.old_string);
            if (n === 0) {
                errors.push(`edit ${i + 1}: old_string not found in ${abs}`);
                return;
            }
            if (n > 1 && !e.replace_all) {
                errors.push(`edit ${i + 1}: old_string appears ${n} times; add context or set replace_all`);
                return;
            }
            applied += e.replace_all ? n : 1;
            let start = text.indexOf(e.old_string);
            while (start !== -1) {
                replacements.push({ start, end: start + e.old_string.length, text: e.new_string, editNumber: i + 1 });
                if (!e.replace_all)
                    break;
                start = text.indexOf(e.old_string, start + e.old_string.length);
            }
        });
        replacements.sort((a, b) => a.start - b.start || a.end - b.end);
        for (let i = 1; i < replacements.length; i++) {
            const previous = replacements[i - 1];
            const current = replacements[i];
            if (current.start < previous.end) {
                errors.push(`edits ${previous.editNumber} and ${current.editNumber} overlap in ${abs}`);
                break;
            }
        }
        if (errors.length > 0) {
            return {
                ok: false,
                output: '',
                error: `No edits applied (${errors.length} problem(s)):\n${errors.map(e => `  ${e}`).join('\n')}`,
            };
        }
        const parts = [];
        let cursor = 0;
        for (const replacement of replacements) {
            parts.push(text.slice(cursor, replacement.start), replacement.text);
            cursor = replacement.end;
        }
        parts.push(text.slice(cursor));
        const updated = parts.join('');
        const bytes = Buffer.byteLength(updated, 'utf8');
        if (bytes > MAX_EDIT_BYTES) {
            return { ok: false, output: '', error: `No edits applied — result is ${bytes} bytes; limit is ${MAX_EDIT_BYTES}.` };
        }
        if (updated === text) {
            return { ok: true, output: `No change to ${abs}; the file already matched.` };
        }
        let checkpoint;
        try {
            checkpoint = replaceProjectFile(abs, updated, ctx, { expected: text });
        }
        catch (err) {
            return { ok: false, output: '', error: `Cannot write ${abs}: ${err.message}` };
        }
        return {
            ok: true,
            output: `Edited ${abs} — applied ${edits.length} edit(s), ${applied} replacement(s).\nRecovery checkpoint: ${checkpoint}`,
        };
    },
};
