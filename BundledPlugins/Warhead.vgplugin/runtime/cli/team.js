import { activeProvider, authHeaders, findProvider, resolveProvider } from '../model/provider.js';
const usage = '/team [on | off | add <name> <provider> <model> [specialty] | remove <name> | check]';
export function teamSummary(cfg) {
    return `team: ${cfg.team.enabled ? 'on' : 'off'} · ${cfg.team.members.length} advisor(s)`;
}
/** Configuration only, except for the explicit check command. Enabling a team
 * never probes, downloads, or loads a model before the next task. */
export async function teamCommand(cfg, args, leadModel) {
    const [command, ...rest] = args;
    if (!command) {
        const lead = activeProvider();
        return { changed: false, lines: [teamSummary(cfg), `lead: ${leadModel ?? '(unavailable)'} @ ${lead.name} (${lead.host})`,
                ...cfg.team.members.map(m => {
                    const provider = findProvider(cfg, m.provider);
                    return `  ${m.name}: ${m.model} @ ${m.provider} (${provider?.host ?? 'provider missing'})${m.role ? ` · ${m.role}` : ''}`;
                }), 'Advisors offer suggestions; the lead retains tool execution and permission checks.', usage] };
    }
    if (command === 'on' || command === 'off') {
        if (rest.length)
            throw new Error(usage);
        if (command === 'on') {
            if (!cfg.team.members.length)
                throw new Error('Add an advisor with /team add <name> <provider> <model> first.');
            for (const member of cfg.team.members) {
                const provider = findProvider(cfg, member.provider);
                if (!provider)
                    throw new Error(`Advisor '${member.name}' needs provider '${member.provider}'. Use /provider add first.`);
                resolveProvider(provider);
            }
        }
        cfg.team.enabled = command === 'on';
        return { changed: true, lines: [teamSummary(cfg)] };
    }
    if (command === 'add') {
        const [name, providerName, model, ...specialty] = rest;
        if (!name || !providerName || !model)
            throw new Error(usage);
        if (!/^[a-zA-Z0-9_-]{1,40}$/.test(name))
            throw new Error('Advisor names use 1–40 letters, numbers, underscores, or hyphens.');
        const provider = findProvider(cfg, providerName);
        if (!provider)
            throw new Error(`Unknown provider '${providerName}'. Register it with /provider add first.`);
        resolveProvider(provider);
        if (/^(?:mlx|gguf|ollama):/.test(model))
            throw new Error('Use an Ollama server model tag, without a native-runtime prefix.');
        const role = specialty.join(' ');
        if (role.length > 500)
            throw new Error('An advisor specialty must be at most 500 characters.');
        const existing = cfg.team.members.findIndex(m => m.name.toLowerCase() === name.toLowerCase());
        if (existing < 0 && cfg.team.members.length >= 4)
            throw new Error('A team supports up to four advisors. Remove one first.');
        const member = { name, provider: provider.name, model, ...(role ? { role } : {}) };
        if (existing < 0)
            cfg.team.members.push(member);
        else
            cfg.team.members[existing] = member;
        return { changed: true, lines: [`${existing < 0 ? 'Added' : 'Updated'} advisor ${name}: ${model} @ ${provider.name}`, teamSummary(cfg)] };
    }
    if (command === 'remove') {
        if (rest.length !== 1)
            throw new Error(usage);
        const index = cfg.team.members.findIndex(m => m.name.toLowerCase() === rest[0].toLowerCase());
        if (index < 0)
            throw new Error(`Unknown advisor '${rest[0]}'.`);
        const [member] = cfg.team.members.splice(index, 1);
        if (!cfg.team.members.length)
            cfg.team.enabled = false;
        return { changed: true, lines: [`Removed advisor ${member.name}`, teamSummary(cfg)] };
    }
    if (command === 'check') {
        if (rest.length)
            throw new Error(usage);
        const entries = [
            ...(leadModel ? [{ name: 'lead', model: leadModel, provider: activeProvider() }] : []),
            ...cfg.team.members.map(m => ({ name: m.name, model: m.model, provider: (() => {
                    const entry = findProvider(cfg, m.provider);
                    return entry ? resolveProvider(entry) : null;
                })() })),
        ];
        const lines = await Promise.all(entries.map(async (entry) => {
            if (!entry.provider)
                return `${entry.name}: provider missing`;
            const provider = entry.provider;
            try {
                const response = await fetch(`${provider.host}/api/show`, { method: 'POST',
                    headers: { 'content-type': 'application/json', ...authHeaders(provider) },
                    body: JSON.stringify({ model: entry.model }), signal: AbortSignal.timeout(10_000) });
                if (!response.ok) {
                    await response.body?.cancel();
                    throw new Error(`HTTP ${response.status}`);
                }
                const body = await response.json();
                if (body.error)
                    throw new Error(body.error);
                return `${entry.name}: available · ${entry.model} @ ${provider.host}`;
            }
            catch (error) {
                return `${entry.name}: unavailable · ${provider.host} · ${error instanceof Error ? error.message : String(error)}`;
            }
        }));
        return { changed: false, lines: [...lines, 'Metadata check only; models load when you submit a task with team mode on.'] };
    }
    throw new Error(usage);
}
