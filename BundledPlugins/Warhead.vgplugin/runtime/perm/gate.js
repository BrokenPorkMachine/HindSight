/**
 * Permission gate.
 *
 * The single choke point for every tool call. Nothing authorizes a tool except
 * this file, which matters because the model is remote and the content it reads
 * is untrusted: a prompt injection inside a source file can cause the model to
 * request a write or a shell command, and this is where that request is
 * evaluated.
 *
 * Two deliberate design choices:
 *
 * 1. Policies are read from config at call time, never captured at startup, so
 *    `/permissions` takes effect on the very next call.
 *
 * 2. "Always allow" is SESSION-scoped by default and is not persisted. Silently
 *    writing "always allow write" into a config file is a footgun that outlives
 *    the session that granted it. Persisting requires an explicit
 *    `/permissions` edit, which is itself a deliberate act.
 */
/** A concise, stable explanation for interactive status surfaces. */
export function describeApprovalMode(mode) {
    switch (mode) {
        case 'strict': return 'ask unless explicitly allowed';
        case 'balanced': return 'allow workspace edits; ask for external actions';
        case 'relaxed': return 'allow unless explicitly denied';
    }
}
/** Shell metacharacters that chain or detach independent commands. A command
 *  containing any of these is split, and EVERY segment must independently
 *  satisfy the allowlist. Without this, `git status; rm -rf ~` would pass a
 *  prefix check.
 *
 *  `&` was MISSING from the first version, so `git status & rm -rf ~` was
 *  auto-approved in full: the whole string stayed one segment, it began with an
 *  allowlisted prefix, and a destructive command rode along in the background.
 *
 *  `&` is split on except after `>`, so ordinary descriptor duplication
 *  (`2>&1`) is not mistaken for a command separator. Mis-splitting only ever
 *  makes the check stricter, which is the safe direction. */
const SEGMENT_SPLIT = /(?:\|\||&&|(?<![><])&|\||;|\n|\r)/;
/** Split a shell command into independently-authorized segments. */
export function splitCommand(command) {
    return command
        .split(SEGMENT_SPLIT)
        .map(s => s.trim())
        .filter(s => s !== '');
}
function normalize(s) {
    return s.replace(/\s+/g, ' ').trim();
}
function diffPreview(oldText, newText) {
    const oldLines = oldText === '' ? [] : oldText.split('\n').map(line => `- ${line}`);
    const newLines = newText === '' ? [] : newText.split('\n').map(line => `+ ${line}`);
    const lines = [...oldLines, ...newLines];
    const maxLines = 40;
    const maxChars = 6000;
    let preview = lines.slice(0, maxLines).join('\n');
    if (preview.length > maxChars)
        preview = preview.slice(0, maxChars);
    const clipped = lines.length > maxLines || oldLines.join('\n').length + newLines.join('\n').length > maxChars;
    return `${preview || '(empty)'}${clipped ? `\n… preview truncated (${lines.length} changed line(s))` : ''}`;
}
/** Does `segment` satisfy an allowlist entry? An entry matches when the segment
 *  equals it or starts with it followed by whitespace — so `git status` allows
 *  `git status --short` but never `git statusx`. */
export function allowlistMatch(segment, allowlist) {
    const seg = normalize(segment);
    for (const raw of allowlist) {
        const entry = normalize(raw);
        if (entry === '')
            continue;
        if (seg === entry)
            return true;
        if (seg.startsWith(entry) && /\s/.test(seg[entry.length] ?? ''))
            return true;
    }
    return false;
}
export function checkBashAllowlist(command, allowlist) {
    const segments = splitCommand(command);
    if (segments.length === 0)
        return { allowed: false, blockedSegment: '(empty command)' };
    for (const seg of segments) {
        if (!allowlistMatch(seg, allowlist))
            return { allowed: false, blockedSegment: seg };
    }
    return { allowed: true };
}
/** Shell constructs that make static allowlist reasoning unsound. If one is
 *  present the command can never be auto-approved, only explicitly allowed. */
const OBFUSCATION = [
    /`/, // command substitution
    /\$\(/, // command substitution
    /\$\(\(/, // nested/subshell substitution
    /\$\{/, // variable expansion
    /\$\([\w@*#?\-!$]/, // variable substitution
    /(^|\s)(\.\s*\/|source\s)/, // sourcing another script
    /(^|\s)eval\s/, // eval
    /(^|\s)eval\s+["']/, // eval with string args
    /\|\s*(sh|bash|zsh|fish|python\d?|node|perl|ruby)\b/, // piping into an interpreter
    /(^|\s)(sh|bash|zsh)\s+-c\b/,
    />/, // redirection can clobber files
    /</, // input redirection can inject files
];
export function hasObfuscation(command) {
    return OBFUSCATION.some(re => re.test(command));
}
function describe(call, tool) {
    const a = call.args;
    const path = typeof a['path'] === 'string' ? a['path'] : undefined;
    if (tool.def.name === 'rea_run_task') {
        const task = typeof a['task'] === 'string' ? a['task'] : '(unknown task)';
        return {
            toolName: tool.def.name,
            summary: `run REA task ${task}`,
            detail: `task: ${task}\narguments:\n${JSON.stringify(a['arguments'] ?? {}, null, 2)}`,
            args: a,
        };
    }
    if (tool.def.name === 'mcp_call_tool') {
        const serverRaw = typeof a['server'] === 'string' ? a['server'] : '';
        const server = serverRaw === '' ? '(unknown server)' : serverRaw;
        const serverSafe = /^[a-zA-Z0-9_\-\.]+$/.test(serverRaw);
        const toolNameRaw = typeof a['tool'] === 'string' ? a['tool'] : '';
        const toolName = toolNameRaw === '' ? '(unknown tool)' : toolNameRaw;
        const toolSafe = /^[a-zA-Z0-9_\-\.]+$/.test(toolNameRaw);
        return {
            toolName: tool.def.name,
            summary: `call MCP tool ${server}.${toolName}`,
            detail: `server: ${server}${!serverSafe ? ' (UNSAFE NAME)' : ''}\ntool: ${toolName}${!toolSafe ? ' (UNSAFE NAME)' : ''}\narguments:\n${JSON.stringify(a['arguments'] ?? {}, null, 2)}`,
            args: a,
        };
    }
    switch (tool.def.name) {
        case 'write':
        case 'create': {
            const content = typeof a['content'] === 'string' ? a['content'] : '';
            const lines = content === '' ? 0 : content.split('\n').length;
            const verb = path ? `write ${lines} line(s) to ${path}` : 'write a file';
            return {
                toolName: tool.def.name,
                summary: verb,
                detail: `--- ${path ?? '?'} (new or overwritten) ---\n${content}`,
                args: a,
            };
        }
        case 'edit':
        case 'str_replace': {
            const oldS = typeof a['old_string'] === 'string' ? a['old_string'] : '';
            const newS = typeof a['new_string'] === 'string' ? a['new_string'] : '';
            const all = a['replace_all'] === true;
            return {
                toolName: tool.def.name,
                summary: `edit ${path ?? '?'}${all ? ' (all occurrences)' : ''}`,
                detail: `--- ${path ?? '?'}${all ? `\nreplace all occurrences` : ''}\n${diffPreview(oldS, newS)}`,
                args: a,
            };
        }
        case 'bash': {
            const cmd = typeof a['command'] === 'string' ? a['command'] : '';
            return {
                toolName: 'bash',
                summary: `run: ${cmd}`,
                detail: `cwd: ${process.cwd()}\n$ ${cmd}`,
                args: a,
            };
        }
        case 'fetch_web_page': {
            const url = typeof a['url'] === 'string' ? a['url'] : '?';
            return {
                toolName: 'fetch_web_page',
                summary: `fetch web page: ${url}`,
                detail: `URL: ${url}\nmax_chars: ${a['max_chars'] ?? 'default'}`,
                args: a,
            };
        }
        case 'web_search': {
            const q = typeof a['query'] === 'string' ? a['query'] : '?';
            return {
                toolName: 'web_search',
                summary: `search web: ${q}`,
                detail: `query: ${q}`,
                args: a,
            };
        }
        default:
            return {
                toolName: tool.def.name,
                summary: `${tool.def.name}(${Object.keys(a).join(', ')})`,
                detail: JSON.stringify(a, null, 2),
                args: a,
            };
    }
}
export class Gate {
    /** Tool names the user chose to allow for the remainder of this session. */
    sessionTools = new Set();
    /** Individual bash commands allowed for this session. */
    sessionCommands = new Set();
    /** Tools the user has explicitly denied; a denial is sticky within a session
     *  so the model cannot simply re-ask the same thing repeatedly. */
    sessionDenied = new Set();
    // Declared explicitly rather than as a constructor parameter property, which
    // is not erasable syntax.
    getConfig;
    constructor(getConfig) {
        this.getConfig = getConfig;
    }
    /** Decided without asking the user. */
    evaluate(call, tool) {
        const cfg = this.getConfig();
        const name = tool.def.name;
        const sessionScope = sessionScopeFor(name, call.args);
        if (this.sessionDenied.has(sessionScope))
            return 'deny';
        if (this.sessionTools.has(sessionScope))
            return 'allow';
        if (cfg.permissions.alwaysAllowAll)
            return 'allow';
        if (cfg.permissions.alwaysAllow.includes(name))
            return 'allow';
        // bash is resolved BEFORE the generic per-tool table because it has its own
        // richer policy (ask | allow | allowlist). A default `tools.bash` entry would
        // otherwise shadow permissions.bash and silently disable the allowlist.
        // `deny` is honoured as an explicit override.
        if (name === 'bash') {
            if (cfg.permissions.tools[name] === 'deny')
                return 'deny';
            const cmd = typeof call.args['command'] === 'string' ? call.args['command'] : '';
            return this.evaluateBash(cmd, cfg.permissions.bash, cfg.permissions.bashAllowlist);
        }
        const policy = cfg.permissions.tools[name];
        if (policy === 'deny')
            return 'deny';
        if (policy === 'allow')
            return 'allow';
        if (policy === 'ask')
            return 'ask';
        return this.defaultVerdict(name, tool.mutates, cfg.permissions.approvalMode);
    }
    /**
     * Baseline policy for tools without a durable exception. `balanced` is
     * intentionally limited to the file-edit primitives: their workspace and
     * symlink checks remain enforced by Registry.dispatch. It never broadens
     * shell, MCP, REA, or other external-capability authority.
     */
    defaultVerdict(name, mutates, mode) {
        if (mode === 'relaxed')
            return 'allow';
        if (!mutates)
            return mode === 'strict' ? 'ask' : 'allow';
        if (mode === 'balanced' && isWorkspaceEdit(name))
            return 'allow';
        return 'ask';
    }
    evaluateBash(cmd, policy, allowlist) {
        if (policy === 'allow')
            return 'allow';
        if (policy === 'ask')
            return 'ask';
        // allowlist
        if (this.sessionCommands.has(normalize(cmd)))
            return 'allow';
        if (hasObfuscation(cmd))
            return 'ask';
        return checkBashAllowlist(cmd, allowlist).allowed ? 'allow' : 'ask';
    }
    request(call, tool) {
        return describe(call, tool);
    }
    /** Record the user's answer. Session scope only — see the file header.
     *
     *  For bash, "always" grants THIS COMMAND, not the tool. Adding 'bash' to
     *  sessionTools would let a single grant on `git status` silently authorize
     *  every later command including `rm -rf`, because the tool-level check
     *  short-circuits ahead of the command-level one. */
    record(toolName, outcome, args) {
        if (outcome === 'allow_session') {
            if (toolName === 'bash') {
                if (typeof args['command'] === 'string') {
                    this.sessionCommands.add(normalize(args['command']));
                }
                return;
            }
            this.sessionTools.add(sessionScopeFor(toolName, args));
            return;
        }
        if (outcome === 'deny')
            this.sessionDenied.add(sessionScopeFor(toolName, args));
    }
    /** Forget session grants. Exposed as /permissions reset. */
    resetSession() {
        this.sessionTools.clear();
        this.sessionCommands.clear();
        this.sessionDenied.clear();
    }
    sessionState() {
        // Shell arguments can contain sensitive paths or values, so show a count
        // rather than re-rendering them in an ordinary status screen.
        return { allow: [...this.sessionTools], commandCount: this.sessionCommands.size, deny: [...this.sessionDenied] };
    }
}
function isWorkspaceEdit(name) {
    return name === 'write' || name === 'create' || name === 'edit' || name === 'str_replace' || name === 'multi_edit';
}
function sessionScopeFor(toolName, args) {
    // The generic REA dispatcher represents hundreds of distinct operations.
    // An "always" decision for one should never authorize the entire catalog.
    if (toolName === 'rea_run_task' && typeof args['task'] === 'string') {
        return `${toolName}:${args['task']}`;
    }
    if (toolName === 'mcp_call_tool' && typeof args['server'] === 'string' && typeof args['tool'] === 'string') {
        return `${toolName}:${args['server']}:${args['tool']}`;
    }
    return toolName;
}
