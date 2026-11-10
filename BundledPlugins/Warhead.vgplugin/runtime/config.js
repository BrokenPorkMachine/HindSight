/**
 * Config: load/save JSON at ~/.config/warhead/config.json.
 *
 * Live-editable at runtime (e.g. `/permissions bash allow`); read at call time
 * rather than captured at startup so changes take effect on the next tool call.
 */
import { readFileSync, existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { writePrivateJson } from './context/storage.js';
export const DEFAULT_CONFIG = {
    team: { enabled: false, members: [], timeoutMs: 180_000, maxReplyTokens: 512 },
    model: {
        /** Discover a tool-capable model on the configured Ollama surfaces. */
        candidates: [],
        /** Offer an interactive picker of tool-capable installed models if all
         *  candidates fail. */
        scanInstalledOnFailure: true,
        /** Recorded probe outcomes, keyed by model, to avoid re-probing known-bad. */
        known: {},
    },
    permissions: {
        // Common inspection/test commands should not interrupt an engineering turn.
        // Anything outside this list remains an explicit decision.
        bash: 'allowlist',
        approvalMode: 'balanced',
        // Allowlist prefixes match any further arguments, so each entry should be a
        // command whose argument space is harmless. `cat` was removed: with no
        // restriction it also permits `cat ~/.ssh/id_rsa`. Use `head`/`tail`, which
        // take an explicit file and default to bounded output.
        bashAllowlist: [
            'git status', 'git diff', 'git log', 'git show', 'git branch',
            'ls', 'pwd', 'wc', 'head', 'tail', 'which',
            'npm test', 'npm run', 'pytest', 'cargo test', 'go test',
        ],
        alwaysAllow: ['read_file', 'glob', 'grep', 'todo'],
        // Deliberately empty: approvalMode is the visible default. Add a tool here
        // when it needs a durable exception (for example, { "bash": "deny" }).
        tools: {},
    },
    transcripts: {
        enabled: true,
        maxBytes: 256 * 1024 * 1024,
        maxToolResultBytes: 8 * 1024,
    },
    limits: {
        maxSteps: 80,
        repeatCallLimit: 3,
        bashTimeoutMs: 120_000,
        toolTimeoutMs: 30_000,
        perToolTimeoutMs: { bash: 120_000, read_file: 10_000 },
        compactThresholdChars: 240_000,
        compactKeepRecent: 8,
    },
    providers: {
        active: null,
        list: [],
    },
    mcp: {
        servers: [
            {
                name: 'tronscan',
                url: 'https://mcp.tronscan.org/mcp',
                enabled: false,
                readOnly: true,
                headers: {
                    'TRON-PRO-API-KEY': '${TRONSCAN_API_KEY}',
                    'Accept': 'application/json, text/event-stream',
                    'Content-Type': 'application/json',
                },
            },
        ],
    },
};
export function configPath() {
    const base = process.env['XDG_CONFIG_HOME'] ?? join(homedir(), '.config');
    return join(base, 'warhead', 'config.json');
}
/** Keep only well-formed provider entries.
 *
 *  The config is hand-editable, and one malformed line used to be fatal: lookups
 *  call `name.trim()`, so a provider missing `name` threw a TypeError that
 *  stopped the tool from starting at all — a bad `host` would not have been
 *  caught here either, only later, at the first request. Dropping the entry is
 *  better than refusing to boot; `/provider` will simply not list it. */
function sanitizeMcpServers(raw) {
    if (!Array.isArray(raw))
        return [];
    const out = [];
    const seen = new Set();
    for (const entry of raw) {
        if (typeof entry !== 'object' || entry === null)
            continue;
        const e = entry;
        const name = typeof e['name'] === 'string' ? e['name'].trim() : '';
        const url = typeof e['url'] === 'string' ? e['url'].trim() : '';
        if (name === '' || url === '')
            continue;
        try {
            const u = new URL(url);
            if (u.protocol !== 'http:' && u.protocol !== 'https:')
                continue;
        }
        catch {
            continue;
        }
        const key = name.toLowerCase();
        if (seen.has(key))
            continue;
        seen.add(key);
        let headers;
        if (e['headers'] && typeof e['headers'] === 'object' && !Array.isArray(e['headers'])) {
            const h = e['headers'];
            headers = {};
            for (const [k, v] of Object.entries(h)) {
                if (typeof v === 'string')
                    headers[k] = v;
            }
            if (Object.keys(headers).length === 0)
                headers = undefined;
        }
        out.push({
            name,
            url,
            enabled: e['enabled'] !== false,
            ...(e['readOnly'] === true ? { readOnly: true } : {}),
            ...(headers ? { headers } : {}),
        });
    }
    return out;
}
function sanitizeProviders(raw) {
    if (!Array.isArray(raw))
        return [];
    const out = [];
    for (const entry of raw) {
        if (typeof entry !== 'object' || entry === null)
            continue;
        const e = entry;
        const name = typeof e['name'] === 'string' ? e['name'].trim() : '';
        const host = typeof e['host'] === 'string' ? e['host'].trim() : '';
        if (name === '' || host === '')
            continue;
        out.push({
            name,
            host,
            ...(typeof e['apiKeyEnv'] === 'string' && e['apiKeyEnv'] !== '' ? { apiKeyEnv: e['apiKeyEnv'] } : {}),
            ...(typeof e['apiKey'] === 'string' && e['apiKey'] !== '' ? { apiKey: e['apiKey'] } : {}),
        });
    }
    return out;
}
/** Hand-edited limits and permissions must fail clearly, never become NaN or
 * silently fall through to a more permissive policy. Omitted fields keep defaults. */
function validateConfigInput(raw) {
    const object = (value, field) => {
        if (!value || typeof value !== 'object' || Array.isArray(value))
            throw new Error(`${field} must be an object`);
        return value;
    };
    const root = object(raw, 'config');
    for (const section of ['model', 'permissions', 'limits', 'transcripts', 'team']) {
        if (root[section] !== undefined)
            object(root[section], section);
    }
    const check = (section, field, valid, expected) => {
        const value = root[section]?.[field];
        if (value !== undefined && !valid(value))
            throw new Error(`${section}.${field} must be ${expected}`);
    };
    const strings = (value) => Array.isArray(value) && value.every(v => typeof v === 'string' && v.trim() !== '');
    const integer = (value, minimum = 1) => typeof value === 'number' && Number.isSafeInteger(value) && value >= minimum;
    check('team', 'enabled', v => typeof v === 'boolean', 'true or false');
    check('team', 'timeoutMs', v => integer(v) && v <= 2_147_483_647, 'a positive integer within the supported range');
    check('team', 'maxReplyTokens', v => integer(v) && v <= 2048, 'an integer from 1 to 2048');
    check('team', 'members', value => {
        if (!Array.isArray(value) || value.length > 4)
            return false;
        const names = new Set();
        return value.every(entry => {
            if (!entry || typeof entry !== 'object' || Array.isArray(entry))
                return false;
            const member = entry;
            if (typeof member['name'] !== 'string' || !/^[a-zA-Z0-9_-]{1,40}$/.test(member['name']))
                return false;
            const name = member['name'].toLowerCase();
            if (names.has(name))
                return false;
            names.add(name);
            return ['provider', 'model'].every(k => typeof member[k] === 'string' && member[k].trim() !== '')
                && !/^(?:mlx|gguf|ollama):/.test(member['model'])
                && (member['role'] === undefined || (typeof member['role'] === 'string' && member['role'].length <= 500));
        });
    }, 'up to four advisors with unique names, provider names, and Ollama server model tags');
    check('model', 'candidates', strings, 'an array of nonempty model names');
    check('model', 'lastUsed', v => typeof v === 'string', 'a model name');
    check('model', 'scanInstalledOnFailure', v => typeof v === 'boolean', 'true or false');
    check('transcripts', 'enabled', v => typeof v === 'boolean', 'true or false');
    for (const field of ['maxBytes', 'maxToolResultBytes'])
        check('transcripts', field, v => integer(v, 0), 'a nonnegative integer');
    for (const field of ['maxSteps', 'repeatCallLimit', 'compactThresholdChars', 'compactKeepRecent', 'bashTimeoutMs', 'toolTimeoutMs']) {
        check('limits', field, v => integer(v) && (!field.endsWith('Ms') || v <= 2_147_483_647), 'a positive integer within the supported range');
    }
    check('permissions', 'bash', v => ['ask', 'allow', 'allowlist'].includes(v), 'ask, allow, or allowlist');
    check('permissions', 'approvalMode', v => ['strict', 'balanced', 'relaxed'].includes(v), 'strict, balanced, or relaxed');
    for (const field of ['bashAllowlist', 'alwaysAllow'])
        check('permissions', field, strings, 'an array of nonempty strings');
    const records = [
        ['permissions', 'tools', v => ['ask', 'allow', 'deny'].includes(v)],
        ['limits', 'perToolTimeoutMs', v => integer(v) && v <= 2_147_483_647],
        ['model', 'known', v => !!v && typeof v === 'object' && typeof v['status'] === 'string' && typeof v['detail'] === 'string'],
    ];
    for (const [section, field, valid] of records) {
        const value = root[section]?.[field];
        if (value === undefined)
            continue;
        for (const [key, entry] of Object.entries(object(value, `${section}.${field}`))) {
            if (!valid(entry))
                throw new Error(`invalid ${section}.${field}.${key}`);
        }
    }
}
export function loadConfig() {
    const p = configPath();
    if (!existsSync(p))
        return structuredClone(DEFAULT_CONFIG);
    try {
        const parsed = JSON.parse(readFileSync(p, 'utf8'));
        validateConfigInput(parsed);
        const defaults = structuredClone(DEFAULT_CONFIG);
        // Merge nested objects as well as top-level sections. A shallow merge meant a
        // file containing only `permissions.tools.bash` silently DROPPED the sibling
        // defaults, leaving `tools.write` undefined — the policy then fell through to
        // the gate's implicit default by accident rather than by configuration, so a
        // future non-default policy would be unprotected with no visible cause.
        const cfg = {
            team: { ...defaults.team, ...parsed.team },
            model: { ...defaults.model, ...parsed.model },
            permissions: {
                ...defaults.permissions,
                ...parsed.permissions,
                tools: { ...defaults.permissions.tools, ...parsed.permissions?.tools },
            },
            transcripts: { ...defaults.transcripts, ...parsed.transcripts },
            limits: { ...defaults.limits, ...parsed.limits,
                perToolTimeoutMs: { ...defaults.limits.perToolTimeoutMs, ...parsed.limits?.perToolTimeoutMs } },
            providers: {
                active: typeof parsed.providers?.active === 'string' ? parsed.providers.active : null,
                list: sanitizeProviders(parsed.providers?.list),
            },
            mcp: {
                servers: sanitizeMcpServers(parsed.mcp && Object.prototype.hasOwnProperty.call(parsed.mcp, 'servers')
                    ? parsed.mcp.servers
                    : DEFAULT_CONFIG.mcp.servers),
            },
        };
        // A pointer to a provider that no longer exists would otherwise sit in the
        // config looking valid, and the tool would quietly fall back to localhost
        // with nothing on screen saying the configured endpoint was ignored.
        cfg.providers.active = cfg.providers.list.find(p => p.name.toLowerCase() === cfg.providers.active?.trim().toLowerCase())?.name ?? null;
        // Migrate only the exact former, unauthenticated built-in default. Explicit
        // gateway URLs and user-supplied credentials remain untouched.
        const legacy = cfg.providers.list.find(p => p.name === 'openrouter' && p.host.replace(/\/$/, '') === 'https://openrouter.ai' && !p.apiKey && !p.apiKeyEnv);
        if (legacy && cfg.providers.active === legacy.name && cfg.model.candidates.length === 1 && cfg.model.candidates[0] === 'openrouter/free' && (!cfg.model.lastUsed || cfg.model.lastUsed === 'openrouter/free')) {
            cfg.providers.list = cfg.providers.list.filter(p => p !== legacy);
            cfg.providers.active = null;
            cfg.model.candidates = [];
            delete cfg.model.lastUsed;
            delete cfg.model.known['openrouter/free'];
        }
        return cfg;
    }
    catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        throw new Error(`config at ${p} is unreadable: ${msg}`);
    }
}
export function saveConfig(cfg) {
    writePrivateJson(configPath(), cfg);
}
export function transcriptDir() {
    const base = process.env['XDG_STATE_HOME'] ?? join(homedir(), '.local', 'state');
    return join(base, 'warhead', 'sessions');
}
