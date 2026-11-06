/** Streaming, tool-call, and model-selection diagnostics.
 *   node src/cli/main.ts --tools
 *   node src/cli/main.ts --scan
 */
import { parseCliArgs } from './args.js';
import { loadConfig } from '../config.js';
import { chatOnce, streamChat } from '../model/client.js';
import { listInstalledModels, scanForToolCapable } from '../model/probe.js';
import { applyConfiguredProvider, activeProvider } from '../model/provider.js';
import { resolveModel } from '../model/select.js';
const READ_TOOL = {
    name: 'read_file',
    description: 'Read a file from the local filesystem',
    parameters: {
        type: 'object',
        properties: { path: { type: 'string', description: 'absolute path' } },
        required: ['path'],
    },
};
const COLOR = Boolean(process.stdout.isTTY && !process.env['NO_COLOR']);
const RESET = COLOR ? '\x1b[0m' : '';
const DIM = COLOR ? '\x1b[2m' : '';
const BOLD = COLOR ? '\x1b[1m' : '';
const GREEN = COLOR ? '\x1b[32m' : '';
const RED = COLOR ? '\x1b[31m' : '';
const YELLOW = COLOR ? '\x1b[33m' : '';
const colorFor = (status) => status === 'ok' ? GREEN : status === 'unsupported' ? YELLOW : RED;
function log(msg = '') {
    process.stdout.write(msg + '\n');
}
async function demoText(model) {
    log(`\n${BOLD}— streamed reply —${RESET}`);
    const messages = [{ role: 'user', content: 'In one sentence, what is a hash table?' }];
    const t0 = Date.now();
    let ttfb = 0;
    for await (const ev of streamChat(model, messages)) {
        if (ev.type === 'text') {
            if (ttfb === 0)
                ttfb = Date.now() - t0;
            process.stdout.write(ev.delta);
        }
        else if (ev.type === 'done') {
            log(`\n${DIM}  ttfb ${ttfb}ms · total ${Date.now() - t0}ms · ${ev.promptEvalCount} in / ${ev.evalCount} out${RESET}`);
        }
    }
}
async function demoTools(model) {
    log(`\n${BOLD}— streamed tool call —${RESET}`);
    const messages = [
        { role: 'user', content: 'Read /etc/hosts using the read_file tool. Use the tool.' },
    ];
    for await (const ev of streamChat(model, messages, { tools: [READ_TOOL] })) {
        if (ev.type === 'tool_call') {
            log(`  ${GREEN}tool_call${RESET} ${BOLD}${ev.call.name}${RESET} ${JSON.stringify(ev.call.args)}`);
        }
        else if (ev.type === 'done') {
            log(`  ${DIM}${ev.promptEvalCount} prompt tokens${RESET}`);
        }
    }
    // Non-streaming path, used by the probe — verify the two agree.
    const once = await chatOnce(model, messages, { tools: [READ_TOOL] });
    log(`  ${DIM}non-streaming: ${once.toolCalls.length} call(s) ` +
        `${once.toolCalls.map(c => `${c.name}${JSON.stringify(c.args)}`).join(', ')}${RESET}`);
}
async function main() {
    const { host: cliHost, model: cliModel, args } = parseCliArgs(process.argv.slice(2), true);
    const cfg = loadConfig();
    // CLI server override takes precedence over configured providers.
    if (cliHost) {
        process.env['OLLAMA_HOST'] = cliHost;
        cfg.providers.active = null;
    }
    // CLI model selection overrides candidates.
    if (cliModel) {
        cfg.model.candidates = [cliModel];
        process.env['WARHEAD_MODEL'] = cliModel;
    }
    // Adopt providers.active before anything touches the network. Without this the
    // diagnostic entrypoint ignored the configured provider entirely: `--scan`
    // listed the local daemon's models while the REPL was talking to something
    // else, which is exactly the confusion you consult a diagnostic to avoid.
    applyConfiguredProvider(cfg);
    // REA integration uses an explicitly installed command and never invokes a
    // package manager. WARHEAD_REA_EXECUTABLE may select another local binary.
    if (args[0] === '--rea' || args[0] === 'rea') {
        const { spawn } = await import('node:child_process');
        const executable = process.env['WARHEAD_REA_EXECUTABLE']?.trim() || 'rea-agents';
        const child = spawn(executable, args.slice(1), { stdio: 'inherit' });
        child.on('exit', code => { process.exitCode = code ?? 0; });
        child.on('error', err => {
            log(`\n${RED}failed to launch REA${RESET}\n${err instanceof Error ? err.message : String(err)}`);
            process.exitCode = 1;
        });
        return;
    }
    if (args.includes('--scan')) {
        const provider = activeProvider();
        const installed = await listInstalledModels();
        log(`${BOLD}${provider.name}${RESET} ${DIM}${provider.host}${RESET}`);
        log(`${BOLD}${installed.length} model(s) available${RESET} ${DIM}(local store + endpoint)${RESET}`);
        const { capable, rejected } = await scanForToolCapable(installed);
        for (const r of [...capable, ...rejected]) {
            log(`  ${colorFor(r.status)}${r.status.padEnd(12)}${RESET} ${r.model.padEnd(44)} ${DIM}${r.latencyMs}ms — ${r.detail}${RESET}`);
        }
        log(`\n  ${capable.length} tool-capable, ${rejected.length} not`);
        return;
    }
    // Shared with the REPL so the fallback chain cannot drift between the two
    // entrypoints. Read-only here: a diagnostic run must not rewrite the user's
    // config, so `persist` is off.
    const { model } = await resolveModel(cfg, {
        persist: false,
        report: r => {
            const c = r.status === 'ok' ? GREEN : r.status === 'unsupported' ? YELLOW : RED;
            log(`  ${c}${r.status.padEnd(12)}${RESET} ${r.model.padEnd(44)} ${DIM}${r.latencyMs}ms — ${r.detail}${RESET}`);
        },
    });
    await demoText(model);
    if (args.includes('--tools'))
        await demoTools(model);
    log(`\n${DIM}diagnostics passed — streaming, tool-call assembly, and model selection work.${RESET}`);
}
main().catch((err) => {
    const msg = err instanceof Error ? err.message : String(err);
    log(`\n${RED}diagnostics failed${RESET}\n${msg}`);
    process.exitCode = 1;
});
