/**
 * Failure fingerprints and recovery strategies.
 *
 * A retry repeats an approach. A recovery strategy changes it. The distinction
 * is enforced here: an attempt that produces the same fingerprint without a
 * repository change rotates the strategy instead of replaying the same sequence.
 * The history lives in the ledger, so compaction and context renewal cannot
 * reset it.
 */
export function classifyFailure(text) {
    const value = text.toLowerCase();
    if (/(?:denied by user|denied by policy|permission|not authorized|workspace scope denied)/.test(value))
        return 'permission';
    if (/(?:exceeds the available context|context budget|context window|too many tokens|truncat)/.test(value))
        return 'context';
    if (/(?:timed out|timeout|econnrefused|econnreset|connection refused|fetch failed|cannot reach|network|503|502|504|429)/.test(value))
        return 'transient';
    if (/(?:empty response|malformed|invalid json|tool call|unknown tool|no text or tool calls)/.test(value))
        return 'model';
    if (/(?:enoent|not found|no such file|missing dependency|cannot find module|eacces)/.test(value))
        return 'environment';
    return 'deterministic';
}
const ABSOLUTE_PATH = /(?:[A-Za-z]:)?(?:[\\/][^\s:()[\]]+)+/g;
const FILE_LOCATION = /\b[\w./-]+\.(?:[cm]?[jt]sx?|json|py|rs|go|md|toml|ya?ml):\d+(?::\d+)?/gi;
const HEX = /\b(?:0x)?[0-9a-f]{8,}\b/gi;
const NUMBER = /\b\d+\b/g;
const QUOTED = /['"`]([^'"`\n]{1,60})['"`]/;
/** A stable description of the *kind* of failure, not its noise. */
export function errorFingerprint(text) {
    const raw = text.trim();
    if (raw === '')
        return 'empty-failure';
    const codes = new Set();
    for (const match of raw.matchAll(/\bTS(\d{4})\b/g))
        codes.add(`ts${match[1]}`);
    for (const match of raw.matchAll(/\b(E[A-Z]{3,}|ERR_[A-Z_]+)\b/g))
        codes.add(match[1].toLowerCase());
    for (const match of raw.matchAll(/\bexit(?:\s+code)?\s*[:=]?\s*(\d+)/gi))
        codes.add(`exit${match[1]}`);
    for (const match of raw.matchAll(/\bHTTP\s+(\d{3})\b/gi))
        codes.add(`http${match[1]}`);
    const lines = raw.split('\n').map(line => line.trim()).filter(Boolean);
    const anchor = lines.find(line => /(?:error|failed|failure|cannot|can't|undefined|not found|timed out|denied)/i.test(line))
        ?? lines[0] ?? raw;
    let normalized = anchor
        .replace(FILE_LOCATION, '<file>')
        .replace(ABSOLUTE_PATH, '<path>')
        .replace(HEX, '<hex>')
        .replace(NUMBER, '#')
        .replace(/\s+/g, ' ')
        .toLowerCase()
        .trim();
    const quoted = normalized.match(QUOTED)?.[1]?.trim().toLowerCase();
    if (quoted && !/<file>|<path>/.test(quoted))
        normalized += `:${quoted.replace(/\s+/g, '-').slice(0, 40)}`;
    const codePart = [...codes].sort().join(',');
    const body = normalized.slice(0, 140);
    return (codePart ? `${codePart}:${body}` : body) || 'unknown-failure';
}
/** Deliberately different diagnostic approaches, not different wording of the
 *  same one. The final entry asks the model to abandon the current hypothesis. */
export const RECOVERY_STRATEGIES = [
    {
        name: 'inspect-failure',
        instruction: 'Read the exact file and symbol named by the failure before changing anything. Confirm what the code currently says instead of assuming the previous edit applied.',
    },
    {
        name: 'compare-definitions',
        instruction: 'Compare the failing symbol with its definition, imports, type declarations and call sites. Look for a signature, export or type mismatch rather than repeating the same edit.',
    },
    {
        name: 'reduce-scope',
        instruction: 'Reproduce the failure with the smallest possible change or command. Do not broaden the edit until the single failing unit is understood.',
    },
    {
        name: 'change-hypothesis',
        instruction: 'The previous hypothesis did not hold. Inspect the surrounding architecture and choose a materially different implementation strategy. Record why the old one failed.',
    },
    {
        name: 'block-or-delegate',
        instruction: 'If no permitted action can make progress, stop editing. Record the blocker precisely, preserve the work, and identify the next actionable step for a human or another worker.',
    },
];
export function strategyFor(index) {
    return RECOVERY_STRATEGIES[Math.min(Math.max(0, index), RECOVERY_STRATEGIES.length - 1)];
}
export function strategyName(index) {
    return strategyFor(index).name;
}
/** Repository movement, verified task completion, or new evidence all count as
 *  progress. A changed error message alone does not. */
export function madeProgress(input) {
    return input.beforeFingerprint !== input.afterFingerprint
        || input.completedAfter > input.completedBefore
        || input.evidenceAfter > input.evidenceBefore;
}
export function isTerminal(status) {
    return status === 'completed' || status === 'blocked' || status === 'failed';
}
export function dependenciesSatisfied(task, byId, dependencies) {
    const required = dependencies.get(task.id) ?? [];
    return required.every(id => byId.get(id)?.status === 'completed');
}
export function blockedDependency(task, byId, dependencies) {
    const required = dependencies.get(task.id) ?? [];
    for (const id of required) {
        const dependency = byId.get(id);
        if (!dependency)
            return `dependency ${id} is missing`;
        if (dependency.status === 'blocked' || dependency.status === 'failed')
            return `dependency ${id} is ${dependency.status}`;
    }
    return null;
}
