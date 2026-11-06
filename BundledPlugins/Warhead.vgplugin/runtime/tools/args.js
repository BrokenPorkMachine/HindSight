/**
 * Shared argument coercion for tool inputs.
 *
 * Models return JSON with loose types even when the schema is explicit — a
 * limit arrives as "50" or 50.0 as often as 50. Coercing here keeps every tool
 * body free of defensive casting.
 */
export class ArgError extends Error {
}
export function reqString(args, key) {
    const v = args[key];
    if (typeof v === 'string' && v.trim() !== '')
        return v;
    if (typeof v === 'number' || typeof v === 'boolean')
        return String(v);
    throw new ArgError(`missing required string parameter '${key}'`);
}
export function optString(args, key) {
    const v = args[key];
    if (typeof v === 'string' && v.trim() !== '')
        return v;
    if (typeof v === 'number')
        return String(v);
    return undefined;
}
export function optInt(args, key, min) {
    const v = args[key];
    if (v == null)
        return undefined;
    const n = typeof v === 'number' ? v : Number.parseInt(String(v), 10);
    if (!Number.isFinite(n))
        throw new ArgError(`parameter '${key}' must be a number, got ${JSON.stringify(v)}`);
    const i = Math.trunc(n);
    return i < min ? min : i;
}
export function optBool(args, key) {
    const v = args[key];
    if (v == null)
        return undefined;
    if (typeof v === 'boolean')
        return v;
    const s = String(v).toLowerCase();
    if (s === 'true' || s === '1' || s === 'yes')
        return true;
    if (s === 'false' || s === '0' || s === 'no')
        return false;
    return undefined;
}
export function reqArray(args, key) {
    const v = args[key];
    if (Array.isArray(v))
        return v;
    throw new ArgError(`missing required array parameter '${key}'`);
}
