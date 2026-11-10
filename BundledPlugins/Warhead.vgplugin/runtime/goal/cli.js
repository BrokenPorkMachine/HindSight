/**
 * Goal-execution CLI.
 *
 * The interactive REPL stays a conversation. These subcommands operate on the
 * durable ledger: they can plan, run, pause, inspect, resume and verify a goal
 * without carrying a chat transcript between invocations.
 *
 * Permission handling is deliberately the same Gate the REPL uses. Verification
 * commands are presented to the gate as a synthetic `bash` call, so an
 * acceptance check can never widen execution authority on its own.
 */
import { hostname } from 'node:os';
import { createInterface } from 'node:readline';
import { dirname, join, resolve } from 'node:path';
import { readFileSync, realpathSync, statSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { loadConfig } from '../config.js';
import { applyConfiguredProvider } from '../model/provider.js';
import { resolveModel } from '../model/select.js';
import { Registry } from '../tools/registry.js';
import { Gate } from '../perm/gate.js';
import { createTodoStore } from '../tools/todo.js';
import { GoalLedger, ledgerPathFor } from './ledger.js';
import { withGoalExecutionLease } from './lease-runtime.js';
import { repoSnapshot, workspaceOverview, detectVerificationCommands } from './repo.js';
import { createPlan } from './planner.js';
import { runGoal } from './supervisor.js';
import { runVerification } from './verify.js';
import { DEFAULT_GOAL_BUDGETS } from './types.js';
import { ArtifactStore } from './artifact-store.js';
import { exportGoalEvidence } from './evidence-export.js';
const TTY = Boolean(process.stdout.isTTY && !process.env['NO_COLOR']);
const C = {
    reset: TTY ? '\x1b[0m' : '', bold: TTY ? '\x1b[1m' : '', dim: TTY ? '\x1b[2m' : '',
    green: TTY ? '\x1b[32m' : '', red: TTY ? '\x1b[31m' : '', yellow: TTY ? '\x1b[33m' : '',
    cyan: TTY ? '\x1b[36m' : '',
};
const HELP = `Warhead goal execution

Usage
  warhead-cli goal run "<objective>" [options]   plan and execute a persistent goal
  warhead-cli goal plan "<objective>" [options]  create a plan without executing it
  warhead-cli goal status [goal-id]              show the latest goal from the ledger
  warhead-cli goal inspect [goal-id]             inspect status, plan, blockers, budget
  warhead-cli goal resume [goal-id] [options]    continue a paused or blocked goal
  warhead-cli goal pause [goal-id]               pause at the next cycle boundary
  warhead-cli goal diagnose [goal-id]            inspect durable state and recovery notes
  warhead-cli goal export [goal-id]              export redacted, integrity-checked evidence JSON
  warhead-cli goal evidence [goal-id]            show sources, hashes, observations, claims (evidence export)
  warhead-cli goal report [goal-id]              generate evidence-linked conclusion (evidence export)
  warhead-cli goal reproduce [goal-id]           show authorized reproduction plan
  warhead-cli goal verify [goal-id] [options]    run acceptance and task verification only
  warhead-cli goal lease [goal-id]                inspect the current execution owner
  warhead-cli goal task-assign <goal-id> <task-id> <participant> [review|propose]
                                                 assign non-executing contributor work
  warhead-cli goal task-release <goal-id> <task-id>
                                                 release an assignment
  warhead-cli goal task-assignments <goal-id>     inspect contributor assignments
  warhead-cli goal recover-lease <goal-id> --ack-uncertain
                                                 recover an expired lease after operator inspection

Options
  --model <tag>          use this model for planning and execution
  --host <url>           use this Ollama endpoint for this process
  --workspace <dir>      operate on this directory (default: current directory)
  --db <path>            use this ledger file instead of the per-workspace default
  --yes                  auto-approve permission prompts (explicit denials still apply)
  --json                 machine-readable output for status/diagnose
  --max-cycles <n>       override the goal cycle budget
  --max-steps <n>        override tool steps per model cycle
  --max-tool-calls <n>   override the goal tool-call budget
  --max-duration <min>   override the wall-clock budget in minutes
  --plan-only            for run: stop after the plan is persisted
  --ack-uncertain        explicitly acknowledge uncertain operations during expired-lease recovery
  -h, --help             show this message

Run \`warhead-cli --help\` for the interactive REPL. Goal execution requires the
same tool-capable model and permission configuration as the REPL.`;
const VALUE_OPTIONS = new Set(['--model', '--ollama-model', '--host', '--ollama-host', '--workspace', '--db', '--max-cycles', '--max-steps', '--max-tool-calls', '--max-duration']);
const FLAG_OPTIONS = new Set(['--yes', '--json', '--plan-only', '--ack-uncertain', '-h', '--help', '--version', '-v']);
function parse(argv) {
    const head = argv[0] ?? '';
    const subCandidate = head === 'goal' ? argv[1] : undefined;
    const hasSubcommand = subCandidate !== undefined && subCandidate !== '' && !subCandidate.startsWith('-');
    const command = hasSubcommand ? subCandidate : (head === 'goal' ? '' : head);
    const rest = hasSubcommand ? argv.slice(2) : argv.slice(1);
    const parsed = {
        command, positional: [], yes: false, json: false, planOnly: false, ackUncertain: false, budgets: {}, help: false,
    };
    // Allow --version/-v passthrough for consistency with warhead-cli
    if (rest.includes('--version') || rest.includes('-v')) {
        parsed.help = true; // will be handled as version
    }
    for (let index = 0; index < rest.length; index++) {
        const token = rest[index];
        if (!token.startsWith('-') || token === '-') {
            parsed.positional.push(token);
            continue;
        }
        if (FLAG_OPTIONS.has(token)) {
            if (token === '--yes')
                parsed.yes = true;
            else if (token === '--ack-uncertain')
                parsed.ackUncertain = true;
            else if (token === '--json')
                parsed.json = true;
            else if (token === '--plan-only')
                parsed.planOnly = true;
            else
                parsed.help = true;
            continue;
        }
        const [name, inline] = token.includes('=') ? [token.slice(0, token.indexOf('=')), token.slice(token.indexOf('=') + 1)] : [token, undefined];
        if (!VALUE_OPTIONS.has(name))
            throw new Error(`unknown option: ${token}. Run 'warhead-cli goal --help'.`);
        const value = inline ?? rest[++index];
        if (!value || value.startsWith('-'))
            throw new Error(`${name} requires a value`);
        const number = (label, minimum) => {
            const n = Number(value);
            if (!Number.isSafeInteger(n) || n < minimum)
                throw new Error(`${label} must be an integer >= ${minimum}`);
            return n;
        };
        switch (name) {
            case '--model':
            case '--ollama-model':
                parsed.model = value.trim();
                break;
            case '--host':
            case '--ollama-host':
                parsed.host = value.trim();
                break;
            case '--workspace':
                parsed.workspace = value;
                break;
            case '--db':
                parsed.db = value;
                break;
            case '--max-cycles':
                parsed.budgets.maxCycles = number(name, 1);
                break;
            case '--max-steps':
                parsed.budgets.maxStepsPerCycle = number(name, 1);
                break;
            case '--max-tool-calls':
                parsed.budgets.maxToolCalls = number(name, 1);
                break;
            case '--max-duration':
                parsed.budgets.maxDurationMs = number(name, 1) * 60_000;
                break;
        }
    }
    return parsed;
}
function log(message = '') {
    process.stdout.write(`${message}\n`);
}
function symbol(status) {
    switch (status) {
        case 'passed':
        case 'tool_ok': return `${C.green}✔${C.reset}`;
        case 'failed':
        case 'tool_fail': return `${C.red}✖${C.reset}`;
        case 'blocked': return `${C.yellow}⊘${C.reset}`;
        case 'unverified': return `${C.yellow}?${C.reset}`;
        default: return `${C.cyan}·${C.reset}`;
    }
}
function short(text, max = 160) {
    const line = text.replace(/\s+/g, ' ').trim();
    return line.length > max ? `${line.slice(0, max - 1)}…` : line;
}
/** One approval surface for tool calls and verification commands. */
class Approvals {
    gate;
    registry;
    auto;
    askable;
    rl = null;
    constructor(gate, registry, auto) {
        this.gate = gate;
        this.registry = registry;
        this.auto = auto;
        this.askable = Boolean(process.stdin.isTTY);
    }
    async ask(prompt) {
        if (!this.askable)
            return null;
        const rl = this.rl ?? (this.rl = createInterface({ input: process.stdin, output: process.stdout }));
        return new Promise(resolveAnswer => {
            let settled = false;
            const done = (value) => {
                if (settled)
                    return;
                settled = true;
                rl.off('close', closed);
                resolveAnswer(value);
            };
            const closed = () => done(null);
            rl.once('close', closed);
            rl.question(prompt, value => done(value));
        });
    }
    async decide(call) {
        const tool = this.registry.get(call.name);
        if (!tool)
            return true; // the registry reports unknown tools to the model
        const verdict = this.gate.evaluate(call, tool);
        if (verdict === 'allow')
            return true;
        if (verdict === 'deny') {
            log(`${C.red}  denied by policy: ${call.name}${C.reset}`);
            return false;
        }
        if (this.auto)
            return true;
        if (!this.askable) {
            log(`${C.yellow}  approval required for ${call.name}, but this run is non-interactive. Re-run with --yes or adjust the permission policy.${C.reset}`);
            return false;
        }
        const request = this.gate.request(call, tool);
        log('');
        log(`${C.yellow}${C.bold}approval required${C.reset} ${request.summary}`);
        for (const line of request.detail.split('\n').slice(0, 24))
            log(`${C.dim}  ${short(line, 180)}${C.reset}`);
        const answer = await this.ask('  [y]es / [n]o / [a]llow this session / [o]nce: ');
        const choice = (answer ?? '').trim().toLowerCase();
        if (choice === 'y' || choice === 'yes' || choice === 'o' || choice === 'once')
            return true;
        if (choice === 'a' || choice === 'always') {
            this.gate.record(call.name, 'allow_session', call.args);
            return true;
        }
        if (answer !== null)
            this.gate.record(call.name, 'deny', call.args);
        return false;
    }
    tool(call) {
        return this.decide(call);
    }
    command(command) {
        return this.decide({ id: `verify-${Date.now().toString(36)}`, name: 'bash', args: { command } });
    }
    close() {
        this.rl?.close();
        this.rl = null;
    }
}
async function runtime(parsed) {
    const workspace = realpathSync(resolve(parsed.workspace ?? process.cwd()));
    if (!statSync(workspace).isDirectory())
        throw new Error(`workspace is not a directory: ${workspace}`);
    const config = structuredClone(loadConfig());
    if (parsed.host) {
        process.env['OLLAMA_HOST'] = parsed.host;
        config.providers.active = null;
    }
    if (parsed.model) {
        config.model.candidates = [parsed.model];
        process.env['WARHEAD_MODEL'] = parsed.model;
    }
    // A supervised MGCA goal must not silently become Team work because an
    // ordinary interactive profile left advisors enabled.
    config.team.enabled = false;
    applyConfiguredProvider(config);
    const resolved = await resolveModel(config, { persist: true });
    const registry = new Registry();
    const ledger = GoalLedger.open(workspace, parsed.db ?? ledgerPathFor(workspace));
    const gate = new Gate(() => config);
    const approvals = new Approvals(gate, registry, parsed.yes);
    return {
        config, model: resolved.model, registry, ledger, workspace, approvals,
        close: () => { approvals.close(); ledger.close(); },
    };
}
function objectiveFrom(parsed) {
    const objective = parsed.positional.join(' ').trim();
    if (objective === '')
        throw new Error('an objective is required');
    return objective;
}
function printPlan(goal, ledger) {
    const tasks = ledger.listTasks(goal.id);
    const acceptance = ledger.listAcceptance(goal.id);
    log(`${C.bold}Goal${C.reset} ${goal.id}`);
    log(`${C.dim}${goal.objective}${C.reset}`);
    log(`Plan: ${tasks.length} task(s), ${acceptance.length} acceptance criterion/criteria`);
    for (const task of tasks) {
        log(`  ${String(task.position + 1).padStart(2)}. ${C.bold}${task.id}${C.reset} ${task.title}`);
        log(`      ${C.dim}policy=${task.policy} depends=${ledger.listDependencies(goal.id).filter(d => d.taskId === task.id).map(d => d.dependsOn).join(',') || 'none'}${C.reset}`);
    }
    for (const criterion of acceptance) {
        log(`  ${C.dim}[${criterion.status}]${C.reset} ${criterion.description}${criterion.command ? ` ${C.dim}($ ${criterion.command})${C.reset}` : ''}`);
    }
}
function printStatus(goal, ledger) {
    const tasks = ledger.listTasks(goal.id);
    const acceptance = ledger.listAcceptance(goal.id);
    const completed = tasks.filter(task => task.status === 'completed').length;
    const passed = acceptance.filter(criterion => criterion.status === 'passed').length;
    const checkpoint = ledger.latestCheckpoint(goal.id);
    log(`${C.bold}${goal.status}${C.reset} ${goal.id} ${C.dim}(${goal.workspace})${C.reset}`);
    log(goal.objective);
    log(`${C.dim}cycles ${goal.cycles}/${goal.budgets.maxCycles} · output ${goal.outputTokens}/${goal.budgets.maxOutputTokens} · tools ${goal.toolCalls}/${goal.budgets.maxToolCalls} · tasks ${completed}/${tasks.length} · acceptance ${passed}/${acceptance.length}${C.reset}`);
    if (goal.blocker)
        log(`${C.yellow}blocker:${C.reset} ${goal.blocker}`);
    if (checkpoint)
        log(`${C.dim}last checkpoint: cycle ${checkpoint.cycle} ${checkpoint.phase}${checkpoint.taskId ? ` ${checkpoint.taskId}` : ''}${checkpoint.note ? ` — ${short(checkpoint.note)}` : ''}${C.reset}`);
    for (const task of tasks) {
        const mark = task.status === 'completed' ? `${C.green}✔${C.reset}`
            : task.status === 'blocked' || task.status === 'failed' ? `${C.red}✖${C.reset}`
                : task.status === 'in_progress' || task.status === 'verification' ? `${C.cyan}~${C.reset}` : ' ';
        log(`  ${mark} ${task.id} ${task.title} ${C.dim}[${task.status}, verification ${task.verification}, attempts ${task.attempts}/${task.maxAttempts}]${C.reset}`);
        if (task.lastError)
            log(`      ${C.dim}${short(task.lastError, 200)}${C.reset}`);
        if (task.nextAction)
            log(`      ${C.dim}next: ${short(task.nextAction, 200)}${C.reset}`);
    }
    if (acceptance.length > 0) {
        log(`${C.bold}Acceptance${C.reset}`);
        for (const criterion of acceptance)
            log(`  ${symbol(criterion.status)} ${criterion.description}${criterion.command ? ` ${C.dim}($ ${criterion.command})${C.reset}` : ''}`);
    }
    const evidence = ledger.listEvidence(goal.id, { limit: 8 });
    if (evidence.length > 0) {
        log(`${C.bold}Recent evidence${C.reset}`);
        for (const item of evidence)
            log(`  ${C.dim}[${item.kind}]${C.reset} ${item.label}${item.exitCode === null ? '' : ` ${C.dim}exit=${item.exitCode}${C.reset}`}`);
    }
}
function printDiagnosis(goal, ledger, workspace) {
    const tasks = ledger.listTasks(goal.id);
    const open = ledger.listOpenIntents(goal.id);
    const fenced = ledger.listReplayFencedIntents(goal.id);
    const events = ledger.listEvents(goal.id, 200);
    const attempts = ledger.listAttempts(goal.id);
    const denied = events.filter(event => event.kind === 'tool.denied');
    const failures = attempts.filter(attempt => attempt.status === 'failed');
    const repo = repoSnapshot(workspace);
    log(`${C.bold}Diagnosis${C.reset} ${goal.id} — ${goal.status}`);
    log(`Repository: ${repo.isRepo ? `${repo.branch ?? 'detached'} @ ${(repo.commit ?? '').slice(0, 12)}` : 'not a git repository'}, ${repo.dirty ? 'modified' : 'clean'}`);
    log(`Cycles: ${goal.cycles}; task attempts: ${tasks.reduce((sum, task) => sum + task.attempts, 0)}; evidence: ${ledger.countEvidence(goal.id)}`);
    if (goal.blocker)
        log(`${C.red}Blocker:${C.reset} ${goal.blocker}`);
    if (open.length > 0) {
        log(`${C.yellow}Uncertain side effects (must not be replayed):${C.reset}`);
        for (const intent of open)
            log(`  ${intent.name} on ${intent.path ?? '(no file target)'} — recorded before hash ${intent.beforeHash ?? 'none'}`);
    }
    else {
        log(`${C.green}No open tool intents.${C.reset}`);
    }
    if (fenced.length > 0) {
        log(`${C.yellow}Persistent replay fences (${fenced.length}; blocked across resumes):${C.reset}`);
        for (const intent of fenced.slice(-8)) {
            log(`  ${intent.name} [${intent.status}] on ${intent.path ?? '(no file target)'} — ${short(intent.error ?? intent.output ?? 'side effect uncertain', 150)}`);
        }
    }
    if (denied.length > 0) {
        log(`${C.yellow}Denied calls (sticky for this goal):${C.reset} ${denied.length}`);
        for (const event of denied.slice(0, 5))
            log(`  ${C.dim}${short(JSON.stringify(event.payload), 160)}${C.reset}`);
    }
    const failed = tasks.filter(task => task.status === 'blocked' || task.status === 'failed');
    if (failed.length > 0) {
        log(`${C.red}Blocked tasks:${C.reset}`);
        for (const task of failed)
            log(`  ${task.id} attempts=${task.attempts}/${task.maxAttempts} strategy=${task.strategyIndex} ${short(task.lastError ?? 'no error recorded', 240)}`);
    }
    if (failures.length > 0) {
        log(`${C.bold}Recent failed attempts${C.reset}`);
        for (const attempt of failures.slice(-8)) {
            log(`  ${C.dim}cycle ${attempt.cycle} ${attempt.taskId}/${attempt.strategy} ${attempt.stopReason ?? '?'}${attempt.progressed ? ' progress' : ''}${C.reset} ${short(attempt.summary, 180)}`);
        }
    }
    log(`${C.bold}Last events${C.reset}`);
    for (const event of events.slice(0, 12))
        log(`  ${C.dim}${event.at}${C.reset} ${event.kind}${event.taskId ? ` ${event.taskId}` : ''} ${C.dim}${short(JSON.stringify(event.payload), 140)}${C.reset}`);
}
function latestGoal(ledger, workspace, id) {
    const goal = id ? ledger.getGoal(id) : ledger.latestGoal(workspace);
    if (!goal)
        throw new Error(id ? `goal not found: ${id}` : 'no goal exists for this workspace');
    return goal;
}
function supervisorPrinter() {
    let textOpen = false;
    const closeText = () => { if (textOpen) {
        log('');
        textOpen = false;
    } };
    return event => {
        switch (event.type) {
            case 'plan':
                closeText();
                log(`${C.cyan}plan${C.reset} ${event.tasks} task(s), ${event.acceptance} acceptance criterion/criteria`);
                if (event.detail)
                    log(`  ${C.dim}${short(event.detail, 300)}${C.reset}`);
                break;
            case 'cycle':
                closeText();
                log(`${C.cyan}cycle ${event.cycle}/${event.max}${C.reset}${event.taskId ? ` ${C.bold}${event.taskId}${C.reset}` : ''}`);
                break;
            case 'task':
                closeText();
                log(`  ${C.bold}${event.task.id}${C.reset} ${event.task.title} ${C.dim}— ${short(event.detail, 160)}${C.reset}`);
                break;
            case 'tool_start':
                closeText();
                log(`  ${C.dim}→ ${event.name} ${short(JSON.stringify(event.args), 180)}${C.reset}`);
                break;
            case 'tool_end':
                closeText();
                log(`  ${event.ok ? symbol('tool_ok') : symbol('tool_fail')} ${event.name}${event.ok ? '' : ` ${C.red}${short(event.output, 160)}${C.reset}`}`);
                break;
            case 'text':
                if (!textOpen) {
                    log(`${C.dim}  │ ${C.reset}`);
                    textOpen = true;
                }
                process.stdout.write(event.delta);
                break;
            case 'verification':
                closeText();
                log(`  ${symbol(event.status)} ${short(event.label, 120)}${event.status === 'passed' ? '' : ` ${C.dim}${short(event.detail, 200)}${C.reset}`}`);
                break;
            case 'recovery':
                closeText();
                log(`  ${C.yellow}↻ recovery${C.reset} ${event.taskId} → ${event.strategy} ${C.dim}${event.fingerprint} (${short(event.detail, 120)})${C.reset}`);
                break;
            case 'checkpoint':
                closeText();
                log(`  ${C.dim}checkpoint ${event.phase} (cycle ${event.cycle})${C.reset}`);
                break;
            case 'error':
                closeText();
                log(`  ${C.red}! ${short(event.message, 240)}${C.reset}`);
                break;
            case 'done':
                closeText();
                log(`${event.status === 'completed' ? C.green : event.status === 'paused' ? C.yellow : C.red}${C.bold}${event.status}${C.reset} — ${event.reason}`);
                break;
            case 'usage':
                break;
        }
    };
}
async function execute(parsed, goal, rt, signal) {
    return withGoalExecutionLease(rt.ledger, goal.id, signal, async (owner) => {
        const outcome = await runGoal({
            ledger: rt.ledger, goalId: goal.id, model: rt.model, config: rt.config,
            registry: rt.registry, signal: owner.signal, assertOwnership: owner.assertOwned, budgets: parsed.budgets,
            approve: call => rt.approvals.tool(call),
            authorizeCommand: command => rt.approvals.command(command),
            onEvent: supervisorPrinter(),
        });
        log('');
        log(`${C.dim}goal ${outcome.status}; cycles ${outcome.cycles}; output ${outcome.usage.outputTokens} token(s); tools ${outcome.usage.toolCalls}${C.reset}`);
        printStatus(outcome.goal, rt.ledger);
        return outcome.status === 'completed' ? 0 : 1;
    });
}
async function verifyOnly(parsed, goal, rt, signal) {
    return withGoalExecutionLease(rt.ledger, goal.id, AbortSignal.any([signal, AbortSignal.timeout(goal.budgets.maxDurationMs)]), async (owner) => {
        const tasks = rt.ledger.listTasks(goal.id);
        const acceptance = rt.ledger.listAcceptance(goal.id);
        const logDir = `${dirname(rt.ledger.path)}/logs`;
        let failed = 0;
        let blocked = 0;
        const runOne = async (label, taskId, commands, files) => {
            const outcome = await runVerification({
                workspace: rt.workspace, commands, files, signal: owner.signal,
                logDir, authorize: command => { owner.assertOwned(); return rt.approvals.command(command); },
                timeoutMs: Math.max(60_000, rt.config.limits.bashTimeoutMs ?? 120_000),
            });
            owner.assertOwned();
            if (owner.signal.aborted)
                throw new Error('goal verification cancelled');
            rt.ledger.addEvidence({
                goalId: goal.id, taskId, attemptId: null, kind: commands.length > 0 ? 'command' : 'file',
                label, detail: outcome.detail,
                exitCode: outcome.results.find(result => result.exitCode !== null)?.exitCode ?? null,
                logPath: outcome.results.find(result => result.logPath)?.logPath ?? null,
            });
            log(`  ${symbol(outcome.status)} ${label}`);
            if (outcome.status !== 'passed')
                log(`    ${C.dim}${short(outcome.detail, 300)}${C.reset}`);
            if (outcome.status === 'failed')
                failed++;
            if (outcome.status === 'blocked')
                blocked++;
            return outcome.status;
        };
        log(`${C.bold}Verifying${C.reset} ${goal.id} ${C.dim}(no task is marked complete by this command)${C.reset}`);
        for (const criterion of acceptance) {
            owner.assertOwned();
            if (owner.signal.aborted)
                throw new Error('goal verification cancelled');
            if (criterion.kind === 'command' && criterion.command) {
                const status = await runOne(criterion.description, null, [criterion.command], []);
                rt.ledger.setAcceptanceStatus(goal.id, criterion.id, status, null);
                continue;
            }
            const passed = tasks.every(task => task.status === 'completed' && task.verification === 'passed');
            rt.ledger.setAcceptanceStatus(goal.id, criterion.id, passed ? 'passed' : 'failed', null);
            log(`  ${symbol(passed ? 'passed' : 'failed')} ${criterion.description}`);
            if (!passed)
                failed++;
        }
        for (const task of tasks) {
            owner.assertOwned();
            if (owner.signal.aborted)
                throw new Error('goal verification cancelled');
            if (task.verifier.commands.length === 0 && task.verifier.files.length === 0)
                continue;
            await runOne(`Task ${task.id}: ${task.title}`, task.id, task.verifier.commands, task.verifier.files);
        }
        log('');
        log(failed === 0 && blocked === 0
            ? `${C.green}verification passed${C.reset} — results are recorded as evidence; run 'resume' to let the supervisor complete the goal.`
            : `${C.red}verification did not pass${C.reset} (${failed} failed, ${blocked} blocked). See evidence in 'status'.`);
        return failed === 0 && blocked === 0 ? 0 : 1;
    });
}
function readVersion() {
    try {
        const here = dirname(fileURLToPath(import.meta.url));
        const root = dirname(dirname(here));
        return JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
    }
    catch {
        try {
            return JSON.parse(readFileSync(join(process.cwd(), 'package.json'), 'utf8')).version;
        }
        catch {
            return 'unknown';
        }
    }
}
async function main(argv) {
    const parsed = parse(argv);
    // Check for version flags
    const rest = argv[0] === 'goal' ? argv.slice(1) : argv;
    if (rest.includes('--version') || rest.includes('-v')) {
        process.stdout.write(`${readVersion()}\n`);
        return 0;
    }
    if (parsed.help && !rest.includes('--version') && !rest.includes('-v')) {
        log(HELP);
        return 0;
    }
    if (parsed.command === '' || parsed.command === 'help') {
        log(HELP);
        return 0;
    }
    // Aliases per VERIFIED_AUTONOMOUS_ENGINEERING_SPEC
    if (parsed.command === 'inspect')
        parsed.command = 'status';
    if (parsed.command === 'evidence')
        parsed.command = 'export';
    if (parsed.command === 'report')
        parsed.command = 'export';
    if (parsed.command === 'reproduce')
        parsed.command = 'export';
    if (!['run', 'plan', 'status', 'resume', 'pause', 'diagnose', 'export', 'verify', 'lease', 'recover-lease'].includes(parsed.command)) {
        throw new Error(`unknown goal command: ${parsed.command}. Run 'warhead-cli goal --help'.`);
    }
    // Status, pause and diagnose read only the ledger. Avoid probing a model for
    // commands that are useful precisely when the model server is unavailable.
    const workspace = realpathSync(resolve(parsed.workspace ?? process.cwd()));
    const ledger = GoalLedger.open(workspace, parsed.db ?? ledgerPathFor(workspace));
    try {
        if (['task-assign', 'task-release', 'task-assignments'].includes(parsed.command)) {
            const goalId = parsed.positional[0];
            if (!goalId)
                throw Error('explicit goal ID required');
            const goal = latestGoal(ledger, workspace, goalId);
            if (goal.workspace !== workspace)
                throw Error('goal belongs to another workspace');
            if (parsed.command === 'task-assignments') {
                const assignments = ledger.listTaskAssignments(goal.id);
                if (parsed.json)
                    log(JSON.stringify(assignments, null, 2));
                else
                    for (const a of assignments)
                        log(`${a.taskId}: ${a.assignee} (${a.scope}, ${a.state})`);
                return 0;
            }
            const taskId = parsed.positional[1];
            if (!taskId)
                throw Error('task ID required');
            const lease = ledger.acquireGoalLease(goal.id, `operator-${process.pid}`, hostname(), process.pid);
            try {
                if (parsed.command === 'task-release') {
                    log(ledger.releaseTaskAssignment(goal.id, taskId, lease.token) ? 'assignment released' : 'no active assignment');
                    return 0;
                }
                const assignee = parsed.positional[2], scope = parsed.positional[3] ?? 'review';
                if (!assignee || (scope !== 'review' && scope !== 'propose'))
                    throw Error('task-assign requires participant and optional review|propose scope');
                const assignment = ledger.assignTask(goal.id, taskId, assignee, 'local-operator', scope, lease.token);
                log(`assigned ${assignment.taskId} to ${assignment.assignee} for ${assignment.scope}; no execution rights granted`);
                return 0;
            }
            finally {
                ledger.releaseGoalLease(goal.id, lease.token);
            }
        }
        if (parsed.command === 'lease' || parsed.command === 'recover-lease') {
            const goal = latestGoal(ledger, workspace, parsed.positional[0]);
            if (parsed.command === 'recover-lease') {
                if (parsed.positional.length !== 1 || !parsed.ackUncertain)
                    throw new Error('recover-lease requires an explicit goal ID and --ack-uncertain');
                const recovered = ledger.recoverExpiredGoalLease(goal.id, true);
                log(recovered ? 'expired goal lease recovered; inspect intents before resume' : 'no lease to recover');
                return 0;
            }
            const lease = ledger.getGoalLease(goal.id);
            if (parsed.json)
                log(JSON.stringify({ goalId: goal.id, lease: lease ? { holder: lease.holder, host: lease.holderHost, pid: lease.holderPid, expiresAtMs: lease.expiresAtMs } : null }, null, 2));
            else
                log(lease ? `Lease for ${goal.id}: ${lease.holder} host=${lease.holderHost} pid=${lease.holderPid} expires=${new Date(lease.expiresAtMs).toISOString()}${Date.now() >= lease.expiresAtMs ? ' (expired; manual recovery required)' : ''}` : `No execution lease for ${goal.id}`);
            return 0;
        }
        if (parsed.command === 'status') {
            const goal = latestGoal(ledger, workspace, parsed.positional[0]);
            if (parsed.json) {
                log(JSON.stringify({ goal, tasks: ledger.listTasks(goal.id), acceptance: ledger.listAcceptance(goal.id), evidence: ledger.listEvidence(goal.id, { limit: 20 }) }, null, 2));
            }
            else
                printStatus(goal, ledger);
            return goal.status === 'completed' ? 0 : 1;
        }
        if (parsed.command === 'diagnose') {
            const goal = latestGoal(ledger, workspace, parsed.positional[0]);
            if (parsed.json) {
                log(JSON.stringify({
                    goal, tasks: ledger.listTasks(goal.id), attempts: ledger.listAttempts(goal.id),
                    openIntents: ledger.listOpenIntents(goal.id), lease: (() => { const x = ledger.getGoalLease(goal.id); return x ? { holder: x.holder, host: x.holderHost, pid: x.holderPid, expiresAtMs: x.expiresAtMs } : null; })(), events: ledger.listEvents(goal.id, 50),
                    repo: repoSnapshot(workspace),
                }, null, 2));
            }
            else
                printDiagnosis(goal, ledger, workspace);
            return goal.status === 'completed' ? 0 : 1;
        }
        if (parsed.command === 'export') {
            const goal = latestGoal(ledger, workspace, parsed.positional[0]);
            const bundle = await exportGoalEvidence(ledger, goal.id, new ArtifactStore(`${dirname(ledger.path)}/artifacts`));
            log(JSON.stringify(bundle, null, 2));
            return 0;
        }
        if (parsed.command === 'pause') {
            const goal = latestGoal(ledger, workspace, parsed.positional[0]);
            if (['completed', 'cancelled', 'failed'].includes(goal.status)) {
                log(`${C.yellow}goal ${goal.id} is already ${goal.status}; nothing to pause${C.reset}`);
                return 0;
            }
            ledger.setGoalStatus(goal.id, 'paused', 'paused by operator via CLI');
            ledger.saveCheckpoint({ goalId: goal.id, cycle: goal.cycles, phase: 'paused', taskId: null, attemptId: null, note: 'operator pause request' });
            log(`paused ${goal.id}. A running supervisor stops at its next cycle boundary; use 'resume' to continue.`);
            return 0;
        }
    }
    finally {
        ledger.close();
    }
    if (parsed.command === 'run' || parsed.command === 'plan') {
        const objective = objectiveFrom(parsed);
        const rt = await runtime(parsed);
        const controller = new AbortController();
        let interrupts = 0;
        const onSignal = () => {
            interrupts++;
            if (interrupts === 1) {
                log(`\n${C.yellow}cancelling: preserving the ledger and stopping at the next safe boundary${C.reset}`);
                controller.abort();
            }
            else {
                process.exit(130);
            }
        };
        process.on('SIGINT', onSignal);
        process.on('SIGTERM', onSignal);
        try {
            const snapshot = repoSnapshot(rt.workspace);
            const overview = workspaceOverview(rt.workspace);
            const hints = detectVerificationCommands(rt.workspace);
            log(`${C.dim}planning with ${rt.model}…${C.reset}`);
            const planned = await createPlan({
                objective, workspace: rt.workspace, model: rt.model, signal: controller.signal,
                maxTasks: parsed.budgets.maxPlanTasks ?? DEFAULT_GOAL_BUDGETS.maxPlanTasks,
                overview, repo: snapshot, hints,
            });
            const plan = { ...planned.plan, objective, ...(planned.plan.fallbackReason ? { fallbackReason: planned.plan.fallbackReason } : {}) };
            if (planned.source === 'heuristic')
                log(`${C.yellow}model plan unavailable; using the conservative heuristic plan: ${plan.fallbackReason ?? 'unknown reason'}${C.reset}`);
            const goal = rt.ledger.createGoal({
                workspace: rt.workspace, objective, branch: snapshot.branch, commit: snapshot.commit,
                budgets: parsed.budgets, plan,
            });
            printPlan(goal, rt.ledger);
            if (parsed.command === 'plan' || parsed.planOnly) {
                log('');
                log(`Plan persisted as ${C.bold}${goal.id}${C.reset}. Execute it with: warhead-cli goal resume ${goal.id}`);
                return 0;
            }
            log('');
            return await execute(parsed, goal, rt, controller.signal);
        }
        finally {
            process.off('SIGINT', onSignal);
            process.off('SIGTERM', onSignal);
            rt.close();
        }
    }
    // resume / verify
    const rt = await runtime(parsed);
    const controller = new AbortController();
    const onSignal = () => { log(`\n${C.yellow}cancelling at the next safe boundary${C.reset}`); controller.abort(); };
    process.on('SIGINT', onSignal);
    process.on('SIGTERM', onSignal);
    try {
        const goal = latestGoal(rt.ledger, rt.workspace, parsed.positional[0]);
        if (parsed.command === 'verify')
            return await verifyOnly(parsed, goal, rt, controller.signal);
        if (goal.status === 'completed') {
            log(`${C.green}goal ${goal.id} is already complete${C.reset}`);
            printStatus(goal, rt.ledger);
            return 0;
        }
        if (goal.status === 'cancelled' || goal.status === 'failed') {
            log(`${C.red}goal ${goal.id} is ${goal.status}; create a new goal instead of resuming it${C.reset}`);
            return 1;
        }
        return await execute(parsed, goal, rt, controller.signal);
    }
    finally {
        process.off('SIGINT', onSignal);
        process.off('SIGTERM', onSignal);
        rt.close();
    }
}
main(process.argv.slice(2)).then(code => { process.exitCode = code; }).catch((error) => {
    log(`${C.red}goal: ${error instanceof Error ? error.message : String(error)}${C.reset}`);
    process.exitCode = 1;
});
