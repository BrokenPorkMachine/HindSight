/**
 * Warhead Community: non-configurable tool boundary.
 *
 * This file exists only in the public HindSight bundle, NOT in the private
 * Warhead build. The model, config file, environment and approval dialog cannot
 * grant a tool excluded by this policy.
 *
 * EARLY PREVIEW: this is a capability gate, not an OS sandbox. It permits only
 * one explicitly scoped filesystem read primitive and the bounded Studio
 * snapshot. Do not distribute a general-purpose autonomous CLI until OS-level
 * isolation and the remaining entrypoint audit have passed.
 */
const PUBLIC_TOOLS = new Set(['todo', 'read_file', 'hindsight_context']);

export function communityToolUnavailable(name) {
    return !PUBLIC_TOOLS.has(name);
}

const SENSITIVE_COMPONENTS = new Set([
    '.ssh', '.aws', '.gnupg', '.kube', '.docker', '.git-credentials',
    '.npmrc', '.netrc', '.env', '.pypirc', 'id_rsa', 'id_ed25519',
    'credentials', 'secrets',
]);

/** Conservative path screening in addition to canonical workspace checks.
 *  It cannot replace OS isolation or content-level secret detection. */
export function sensitiveCommunityPath(path) {
    if (typeof path !== 'string')
        return true;
    const components = path.replace(/\\/g, '/').toLowerCase().split('/');
    return components.some(part =>
        SENSITIVE_COMPONENTS.has(part) ||
        part.startsWith('.env.') ||
        /\.(?:pem|p12|pfx|key)$/i.test(part)
    );
}

/** Return a reason when a public-edition tool call must fail closed. */
export function communityToolDenyReason(name, tool, args = {}, ctx = {}) {
    if (communityToolUnavailable(name))
        return 'Not available in Warhead Community preview. Shell, mutation, external network, MCP, REA, and arbitrary registered tools require additional isolation and release qualification.';
    if (!tool || tool.def?.name !== name)
        return 'Unrecognized tool implementation';
    if (name === 'todo')
        return undefined; // In-memory task tracking; no host mutation.
    if (tool.mutates)
        return 'Community inspection tools must be read-only';
    if (name === 'hindsight_context') {
        // The Studio extension creates this in-memory snapshot. The unrelated
        // generic file-based export reader is deliberately not available yet.
        if (ctx.communitySnapshot !== true || args.path !== undefined)
            return 'Only the bounded in-memory HindSight Studio snapshot is available';
        return undefined;
    }
    if (name === 'read_file') {
        if (typeof ctx.workspaceRoot !== 'string' || !ctx.workspaceRoot.trim() ||
            typeof ctx.cwd !== 'string' || !ctx.cwd.trim())
            return 'File access requires a selected, authorized workspace root';
        if (typeof args.path !== 'string' || !args.path.trim())
            return 'File access requires an explicit path';
        if (sensitiveCommunityPath(args.path))
            return 'Sensitive paths are blocked by the Community preview';
        return undefined; // Registry applies canonical root + symlink checks.
    }
    return 'Tool excluded from the Community preview';
}
