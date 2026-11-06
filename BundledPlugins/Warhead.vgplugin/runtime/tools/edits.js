/**
 * Edit-argument coercion.
 *
 * Observed against gpt-oss-120b: it reliably emits `old`/`new` (or
 * `oldStr`/`newStr`) rather than the `old_string`/`new_string` the schema names,
 * so a strict reading rejected the whole call and burned a turn. Being liberal in
 * what we accept is the difference between one retry and a stalled run — the
 * schema stays authoritative for what we advertise, this handles what arrives.
 */
const OLD_KEYS = ['old_string', 'oldString', 'oldStr', 'old', 'from', 'search', 'pattern'];
const NEW_KEYS = ['new_string', 'newString', 'newStr', 'new', 'to', 'replace', 'replacement'];
const ALL_KEYS = [...OLD_KEYS, ...NEW_KEYS, 'replace_all', 'replaceAll', 'all'];
function pick(obj, keys) {
    for (const k of keys) {
        const v = obj[k];
        if (typeof v === 'string')
            return v;
    }
    return undefined;
}
/** Returns null when the object does not look like an edit at all. */
export function coerceEdit(raw) {
    if (typeof raw === 'string')
        return null;
    if (typeof raw !== 'object' || raw === null)
        return null;
    const o = raw;
    const oldString = pick(o, OLD_KEYS);
    const newString = pick(o, NEW_KEYS);
    if (typeof oldString !== 'string' || typeof newString !== 'string')
        return null;
    const replaceAll = o['replace_all'] === true || o['replaceAll'] === true || o['all'] === true;
    return { oldString, newString, replaceAll };
}
/** A message that names the accepted keys, shown when nothing usable was found. */
export const EDIT_SHAPE_HELP = `each edit needs an old/new string pair. Accepted key names: ` +
    `old_string (or oldString, oldStr, old, from) and new_string (or newString, newStr, new, to), ` +
    `plus optional replace_all. Example: {"old_string":"foo","new_string":"bar"}. ` +
    `Keys seen instead: {keys}.`;
export function describeKeys(raw) {
    if (typeof raw === 'object' && raw !== null) {
        return Object.keys(raw).join(', ') || '(none)';
    }
    return typeof raw;
}
export { ALL_KEYS as EDIT_ARG_KEYS };
