/**
 * Goal supervisor.
 *
 * `runTurn` owns one bounded model conversation. This module owns the goal: it
 * schedules ready tasks, assembles a fresh context per cycle, verifies results,
 * records evidence, rotates recovery strategies and decides when the objective
 * is actually complete.
 *
 * The loop's `answered` stop reason is treated as a *claim*. A goal completes
 * only when its tasks are verified and its acceptance criteria pass.
 */
import { dirname, relative } from 'node:path';
import { createSupervisedTodoStore } from '../tools/todo.js';
import { callSignature, runTurn } from '../agent/loop.js';
import { fetchModelContextLength } from '../model/modelfile.js';
import { assembleContext, cyclePrompt } from './context.js';
import { blockedDependency, classifyFailure, dependenciesSatisfied, errorFingerprint, madeProgress, RECOVERY_STRATEGIES, strategyFor, strategyName, } from './recovery.js';
import { hashBytes, hashFile, repoSnapshot, resolveInside } from './repo.js';
import { runVerification } from './verify.js';
import { ArtifactStore } from './artifact-store.js';
/** The goal ledger is the source of truth. A model's scratch todos never
 * mark ledger tasks complete, and resuming a goal reconstructs the same list. */
export function goalTodoSnapshot(tasks, activeTaskId = null) {
    return tasks.map(task => ({
        content: `${task.id}: ${task.title}`,
        status: task.status === 'completed' && task.verification === 'passed'
            ? 'completed'
            : task.id === activeTaskId ? 'in_progress' : 'pending',
    }));
}
export function withBudgets(goal, overrides) {
    return { ...goal.budgets, ...overrides };
}
/** Convert a persisted intent back into the signature the loop uses for its
 *  replay block. `args` is stored as JSON text, not a live object. */
function intentSignature(intent) {
    try {
        const parsed = JSON.parse(intent.args);
        return callSignature({ id: 'reconciled', name: intent.name, args: parsed });
    }
    catch {
        return `${intent.name}(unparsed)`;
    }
}
/** Load ledger state and reconcile anything a previous process left uncertain.
 *  Safe to call on a live goal: with no open intents it only re-checks recorded
 *  task outputs. */
export function reconcileGoal(ledger, goalId, workspace, onEvent = () => { }) {
    const blockedMutations = new Set();
    const notes = [];
    // Denied and uncertain mutation signatures must survive EVERY resume, not
    // just the first reconciliation. A previous run may already have closed a
    // started intent as unknown; open-intent queries no longer return it.
    for (const event of ledger.listEvents(goalId, 5_000)) {
        if (event.kind !== 'tool.denied' && event.kind !== 'tool.uncertain')
            continue;
        const signature = event.payload['signature'];
        if (typeof signature === 'string')
            blockedMutations.add(signature);
    }
    // The event window is bounded, so include persisted unknown intents as well.
    // Do not allow a long-running goal to age out a replay fence.
    for (const intent of ledger.listReplayFencedIntents(goalId)) {
        if (intent.status === 'unknown' || intent.status === 'denied') {
            blockedMutations.add(intentSignature(intent));
        }
    }
    const open = ledger.listOpenIntents(goalId);
    for (const intent of open) {
        const detail = [`${intent.name}(${intent.args}) was started before the process stopped.`];
        if (intent.path) {
            const absolute = resolveInside(workspace, intent.path);
            const current = absolute ? hashFile(absolute) : null;
            const changed = current !== intent.beforeHash;
            detail.push(`Current content hash: ${current ?? 'missing'}. Recorded before hash: ${intent.beforeHash ?? 'none'}. ${changed ? 'The file changed: the mutation may have committed.' : 'The file is unchanged from the recorded pre-mutation state.'}`);
        }
        else {
            detail.push('The operation has no tracked file target; its side effect is uncertain.');
        }
        detail.push('Do not replay this operation. Inspect current state and propose a different action if needed.');
        ledger.finishIntent(intent.id, 'unknown', null, detail.join(' '), 'interrupted before a durable result');
        ledger.appendEvent(goalId, intent.taskId, intent.attemptId, 'tool.uncertain', {
            signature: intentSignature(intent), name: intent.name, intentId: intent.id,
        });
        ledger.addEvidence({
            goalId, taskId: intent.taskId, attemptId: intent.attemptId,
            kind: 'reconciliation', label: `Uncertain side effect: ${intent.name}`,
            detail: detail.join('\n'), exitCode: null, logPath: null,
        });
        blockedMutations.add(intentSignature(intent));
        notes.push(`uncertain mutation: ${intent.name}`);
    }
    for (const task of ledger.listTasks(goalId)) {
        if (task.status === 'in_progress' || task.status === 'verification') {
            ledger.updateTask(task.goalId, task.id, {
                status: 'pending',
                nextAction: task.nextAction ?? 'Resume from the last verified checkpoint. Inspect current state before acting.',
            });
            ledger.appendEvent(goalId, task.id, null, 'task.resumed', { previous: task.status });
            notes.push(`re-queued interrupted task ${task.id}`);
        }
        if (task.status !== 'completed')
            continue;
        for (const file of ledger.listTaskFiles(goalId, task.id)) {
            const absolute = resolveInside(workspace, file.path);
            const current = absolute ? hashFile(absolute) : null;
            if (file.afterHash === current)
                continue;
            ledger.addEvidence({
                goalId, taskId: task.id, attemptId: null, kind: 'external_change',
                label: `External change after completion: ${file.path}`,
                detail: `Recorded hash ${file.afterHash ?? 'none'}; current hash ${current ?? 'missing'}. The verified output changed outside this goal.`,
                exitCode: null, logPath: null,
            });
            ledger.updateTask(task.goalId, task.id, {
                status: 'pending', verification: 'unverified',
                nextAction: `External change detected in ${file.path}. Re-inspect and re-verify before relying on the earlier completion.`,
            });
            notes.push(`re-opened ${task.id}: ${file.path} changed externally`);
        }
    }
    if (notes.length > 0) {
        const first = ledger.listTasks(goalId)[0];
        if (first)
            onEvent({ type: 'task', task: first, detail: `resume: ${notes.join('; ')}` });
    }
    return { blockedMutations, notes };
}
export function pickNextTask(tasks, dependencies) {
    const byId = new Map(tasks.map(task => [task.id, task]));
    const incomplete = tasks.filter(task => task.status !== 'completed');
    if (incomplete.length === 0)
        return { kind: 'verify_goal' };
    const active = tasks.find(task => task.status === 'in_progress' || task.status === 'verification');
    if (active)
        return { kind: 'task', task: active };
    const ready = tasks
        .filter(task => (task.status === 'pending' || task.status === 'ready') && dependenciesSatisfied(task, byId, dependencies))
        .sort((a, b) => a.position - b.position)[0];
    if (ready)
        return { kind: 'task', task: ready };
    const waiting = [];
    for (const task of tasks) {
        if (task.status !== 'pending' && task.status !== 'ready')
            continue;
        const reason = blockedDependency(task, byId, dependencies);
        waiting.push(reason ? `${task.id}: ${reason}` : `${task.id}: waiting on dependencies ${(dependencies.get(task.id) ?? []).join(', ')}`);
    }
    const failed = tasks.filter(task => task.status === 'blocked' || task.status === 'failed');
    if (failed.length > 0) {
        return {
            kind: 'blocked',
            reason: `No independent work remains. ${failed.map(task => `${task.id} ${task.status}: ${task.lastError ?? 'no reason recorded'}`).join(' | ')}`,
        };
    }
    return { kind: 'blocked', reason: `Dependency graph cannot advance. ${waiting.join(' | ')}` };
}
function toolTarget(call, workspace) {
    const raw = call.args['path'];
    if (typeof raw !== 'string' || raw === '')
        return null;
    return resolveInside(workspace, raw);
}
function claimOf(messages) {
    return messages.findLast(message => message.role === 'assistant' && !message.toolCalls?.length)?.content.trim() ?? '';
}
function firstExit(outcome) {
    return outcome.results.find(result => result.exitCode !== null)?.exitCode ?? null;
}
function firstLog(outcome) {
    return outcome.results.find(result => result.logPath)?.logPath ?? null;
}
function verificationFailure(outcome) {
    const failed = outcome.results.filter(result => !result.ok && !result.blocked);
    const missing = outcome.fileChecks.filter(check => !check.exists);
    return [
        ...failed.map(result => `$ ${result.command}\nexit ${result.exitCode ?? 'none'}${result.timedOut ? ' (timeout)' : ''}\n${result.output}`),
        ...missing.map(check => `required file missing: ${check.path}`),
    ].join('\n\n') || outcome.detail;
}
export async function runGoal(options) {
    const emit = options.onEvent ?? (() => { });
    const turn = options.deps?.turn ?? runTurn;
    const verify = options.deps?.verify ?? runVerification;
    const snapshot = options.deps?.snapshot ?? repoSnapshot;
    const fetchTokens = options.deps?.fetchContextTokens ?? fetchModelContextLength;
    const clock = options.deps?.now ?? (() => Date.now());
    const startedAt = clock();
    const logDir = options.logDir ?? `${dirname(options.ledger.path)}/logs`;
    options.assertOwnership?.();
    const initial = options.ledger.getGoal(options.goalId);
    if (!initial)
        throw new Error(`goal ${options.goalId} not found`);
    const budgets = withBudgets(initial, options.budgets);
    const workspace = initial.workspace;
    const reconciled = reconcileGoal(options.ledger, initial.id, workspace, emit);
    const blockedMutations = reconciled.blockedMutations;
    if (initial.status !== 'completed' && initial.status !== 'cancelled' && initial.status !== 'failed') {
        options.ledger.setGoalStatus(initial.id, 'running', null);
    }
    const goalStart = options.ledger.getGoal(initial.id);
    const initialTasks = options.ledger.listTasks(initial.id);
    emit({ type: 'plan', tasks: initialTasks.length, acceptance: options.ledger.listAcceptance(initial.id).length,
        detail: initialTasks.map(task => `${task.id} ${task.title}`).join('; ') });
    let modelContextTokens = null;
    let contextProbe = null;
    let tokenRatio;
    let cycle = goalStart.cycles;
    let outputTokens = goalStart.outputTokens;
    let toolCalls = goalStart.toolCalls;
    let remediationCount = initialTasks.filter(task => task.id.startsWith('TR')).length;
    // Preserve stable todo ids during execution; reconstruct statuses from the
    // persisted ledger each cycle so model claims cannot overwrite verified state.
    const goalTodos = createSupervisedTodoStore();
    const finish = (finalStatus, finalReason) => {
        const goal = options.ledger.getGoal(initial.id);
        emit({ type: 'done', status: finalStatus, reason: finalReason });
        return {
            goal, status: finalStatus, reason: finalReason, cycles: goal.cycles,
            tasks: options.ledger.listTasks(initial.id),
            evidence: options.ledger.listEvidence(initial.id),
            usage: { outputTokens: goal.outputTokens, toolCalls: goal.toolCalls },
        };
    };
    const stopWith = (statusWhenStopped, message) => {
        options.assertOwnership?.();
        options.ledger.setGoalStatus(initial.id, statusWhenStopped, message);
        options.ledger.saveCheckpoint({ goalId: initial.id, cycle, phase: 'stopped', taskId: null, attemptId: null, note: message });
        return finish(statusWhenStopped, message);
    };
    const runGate = async (label, scope, taskId, attemptId, commands, files) => {
        options.assertOwnership?.();
        const outcome = await verify({
            workspace, commands, files, signal: options.signal, logDir,
            authorize: async (command) => { options.assertOwnership?.(); return options.authorizeCommand(command); },
            timeoutMs: Math.max(60_000, options.config.limits.bashTimeoutMs ?? 120_000),
        });
        options.assertOwnership?.();
        const evidence = commands.length > 0 || files.length > 0
            ? await (async () => {
                const logPath = firstLog(outcome);
                const logArtifact = logPath ? await new ArtifactStore(`${dirname(options.ledger.path)}/artifacts`).putFile(logPath, options.signal) : null;
                return options.ledger.addEvidence({
                    goalId: initial.id, taskId, attemptId,
                    kind: scope === 'goal' ? 'test' : 'command', label,
                    detail: outcome.detail, exitCode: firstExit(outcome), logPath,
                    artifactSha256: logArtifact?.sha256 ?? null,
                    workspaceRevision: snapshot(workspace).commit,
                    provenance: {
                        verifier: 'src/goal/verify.ts',
                        verifierVersion: '1',
                        authorization: outcome.results.map(result => ({
                            commandSha256: hashBytes(result.command),
                            authorized: !result.blocked,
                            exitCode: result.exitCode,
                            timedOut: result.timedOut,
                        })),
                        fileChecks: outcome.fileChecks.map(check => ({ path: check.path, exists: check.exists, sha256: check.hash })),
                        environment: { node: process.version, platform: process.platform, arch: process.arch },
                    },
                });
            })()
            : null;
        emit({ type: 'verification', scope, label, status: outcome.status, detail: outcome.detail });
        return { status: outcome.status, detail: outcome.detail, ...(evidence ? { evidenceId: evidence.id } : {}) };
    };
    const verifyTask = async (task, attemptId) => {
        if (task.policy === 'command') {
            if (task.verifier.commands.length === 0 && task.verifier.files.length === 0) {
                return { status: 'unverified', detail: 'Task declares command verification but configures no command or file check.' };
            }
            return runGate(`Task ${task.id}: ${task.title}`, 'task', task.id, attemptId, task.verifier.commands, task.verifier.files);
        }
        if (task.policy === 'none') {
            return { status: 'unverified', detail: 'Task explicitly disabled verification; the supervisor will not report it as verified.' };
        }
        // An observation proves that a tool returned something, not that the
        // engineering objective is correct. Preserve it for human review, but
        // never let a model's claim + one successful tool call self-certify.
        const intents = options.ledger.listIntents(initial.id, { attemptId, limit: 200 });
        const successful = intents.filter(intent => intent.status === 'completed' && intent.ok);
        if (successful.length === 0) {
            return { status: 'unverified', detail: 'No successful tool observation was recorded; a model claim is not verification.' };
        }
        const evidence = options.ledger.addEvidence({
            goalId: initial.id, taskId: task.id, attemptId, kind: 'observation',
            label: `Task ${task.id}: observations awaiting independent verification`,
            detail: successful.slice(-10).map(intent => `${intent.name}(${intent.args.slice(0, 300)}) -> ${intent.output.replace(/\s+/g, ' ').slice(0, 300)}`).join('\n'),
            exitCode: null, logPath: null,
        });
        emit({ type: 'verification', scope: 'task', label: `Task ${task.id} evidence`, status: 'blocked',
            detail: 'Successful observations were preserved, but evidence-only tasks require independent review or a configured verification check.' });
        return {
            status: 'blocked',
            detail: 'Evidence-only task requires independent verification or explicit human review; observations alone cannot complete it.',
            evidenceId: evidence.id,
        };
    };
    const verifyGoalAcceptance = async () => {
        const tasks = options.ledger.listTasks(initial.id);
        const acceptance = options.ledger.listAcceptance(initial.id);
        const failures = [];
        let blocked = false;
        if (tasks.length === 0) {
            return { done: false, status: 'blocked', reason: 'goal has no tasks and cannot be independently verified' };
        }
        // A passing goal-level command cannot bypass an incomplete or unverified
        // task; the verifier owns completion, not the model or a single check.
        if (!tasks.every(task => task.status === 'completed' && task.verification === 'passed' && task.evidenceId)) {
            return { done: false, status: 'blocked', reason: 'one or more goal tasks lack independently verified completion evidence' };
        }
        if (acceptance.length === 0) {
            const passed = tasks.every(task => task.status === 'completed' && task.verification === 'passed');
            if (passed) {
                const evidence = options.ledger.addEvidence({
                    goalId: initial.id, taskId: null, attemptId: null, kind: 'observation',
                    label: 'Goal accepted: all tasks verified', detail: `${tasks.length} task(s) completed with passed verification.`,
                    exitCode: null, logPath: null,
                });
                options.ledger.completeGoal(initial.id, evidence.id);
                return { done: true, status: 'completed', reason: 'all tasks verified' };
            }
            return { done: false, status: 'blocked', reason: 'goal has no acceptance criteria and not every task is verified' };
        }
        for (const criterion of acceptance) {
            if (criterion.kind === 'command' && criterion.command) {
                const outcome = await runGate(`Goal: ${criterion.description}`, 'goal', null, null, [criterion.command], []);
                options.ledger.setAcceptanceStatus(initial.id, criterion.id, outcome.status, outcome.evidenceId ?? null);
                if (outcome.status === 'blocked')
                    blocked = true;
                if (outcome.status !== 'passed')
                    failures.push(`${criterion.description}: ${outcome.detail}`);
                continue;
            }
            const passed = tasks.every(task => task.status === 'completed' && task.verification === 'passed');
            options.ledger.setAcceptanceStatus(initial.id, criterion.id, passed ? 'passed' : 'failed', null);
            if (!passed)
                failures.push(`${criterion.description}: not every task is verified`);
        }
        if (failures.length === 0) {
            const evidence = options.ledger.addEvidence({
                goalId: initial.id, taskId: null, attemptId: null, kind: 'diff',
                label: 'Goal acceptance verified',
                detail: acceptance.map(criterion => `[${criterion.status}] ${criterion.description}`).join('\n'),
                exitCode: null, logPath: null,
            });
            options.ledger.completeGoal(initial.id, evidence.id);
            return { done: true, status: 'completed', reason: `${acceptance.length} acceptance criterion/criteria passed` };
        }
        if (blocked) {
            return { done: false, status: 'blocked', reason: `acceptance could not be verified because a command was not authorized: ${failures.join(' | ')}` };
        }
        if (remediationCount >= budgets.maxRemediationTasks) {
            return { done: false, status: 'blocked', reason: `acceptance failed and the remediation budget is exhausted: ${failures.join(' | ')}` };
        }
        remediationCount++;
        const id = `TR${String(remediationCount).padStart(3, '0')}`;
        const failingCommands = acceptance
            .filter(criterion => criterion.kind === 'command' && criterion.command && criterion.status !== 'passed')
            .map(criterion => criterion.command);
        options.ledger.addRemediationTask(initial.id, {
            id, position: tasks.length + 1, milestone: 'remediate',
            title: `Repair failing acceptance: ${failures[0].split('\n')[0].slice(0, 120)}`,
            description: `Goal-level acceptance failed. Diagnose the cause, change approach if an earlier fix failed, and satisfy:\n${failures.join('\n')}`,
            policy: failingCommands.length > 0 ? 'command' : 'evidence',
            verifier: { commands: failingCommands, files: [], checks: ['The failing acceptance criteria pass'] },
            maxAttempts: budgets.maxTaskAttempts,
        }, tasks.filter(task => task.status === 'completed').map(task => task.id));
        options.ledger.setGoalStatus(initial.id, 'running', null);
        options.ledger.appendEvent(initial.id, id, null, 'goal.remediation', { failures: failures.slice(0, 5) });
        emit({ type: 'verification', scope: 'goal', label: 'Acceptance failed; remediation scheduled', status: 'failed', detail: failures.join('\n') });
        return { done: false, status: 'running', reason: 'remediation scheduled' };
    };
    while (cycle < budgets.maxCycles) {
        options.assertOwnership?.();
        if (options.signal.aborted)
            return stopWith('paused', 'cancelled by user');
        if (clock() - startedAt > budgets.maxDurationMs) {
            return stopWith('blocked', `goal time budget exhausted after ${Math.round((clock() - startedAt) / 1000)}s`);
        }
        if (outputTokens >= budgets.maxOutputTokens) {
            return stopWith('blocked', `goal output-token budget exhausted (${outputTokens}/${budgets.maxOutputTokens})`);
        }
        if (toolCalls >= budgets.maxToolCalls) {
            return stopWith('blocked', `goal tool-call budget exhausted (${toolCalls}/${budgets.maxToolCalls})`);
        }
        const current = options.ledger.getGoal(initial.id);
        if (current?.status === 'paused')
            return finish('paused', current.blocker ?? 'paused');
        const tasks = options.ledger.listTasks(initial.id);
        const dependencies = new Map();
        for (const dependency of options.ledger.listDependencies(initial.id)) {
            const list = dependencies.get(dependency.taskId) ?? [];
            list.push(dependency.dependsOn);
            dependencies.set(dependency.taskId, list);
        }
        const picked = pickNextTask(tasks, dependencies);
        if (picked.kind === 'blocked')
            return stopWith('blocked', picked.reason);
        if (picked.kind === 'verify_goal') {
            const verdict = await verifyGoalAcceptance();
            if (verdict.done)
                return finish(verdict.status, verdict.reason);
            if (verdict.status === 'blocked')
                return finish('blocked', verdict.reason);
            continue;
        }
        const task = options.ledger.getTask(picked.task.id, initial.id);
        cycle++;
        options.ledger.bumpGoal(initial.id, { cycles: 1 });
        emit({ type: 'cycle', cycle, max: budgets.maxCycles, taskId: task.id });
        if (!contextProbe) {
            contextProbe = (async () => {
                try {
                    modelContextTokens = await fetchTokens(options.model, options.signal);
                }
                catch {
                    modelContextTokens = null;
                }
            })();
        }
        await contextProbe;
        const strategyIndex = Math.min(task.strategyIndex, RECOVERY_STRATEGIES.length - 1);
        const attempt = options.ledger.recordAttempt({
            goalId: initial.id, taskId: task.id, cycle, strategy: strategyName(strategyIndex),
            startedAt: new Date().toISOString(),
        });
        options.ledger.updateTask(task.goalId, task.id, { status: 'in_progress' });
        emit({ type: 'task', task: { ...task, status: 'in_progress' }, detail: `cycle ${cycle}: ${strategyName(strategyIndex)}` });
        options.ledger.saveCheckpoint({ goalId: initial.id, cycle, phase: 'task_start', taskId: task.id, attemptId: attempt.id, note: task.title });
        emit({ type: 'checkpoint', cycle, phase: 'task_start', taskId: task.id });
        const before = snapshot(workspace);
        const completedBefore = tasks.filter(candidate => candidate.status === 'completed').length;
        const evidenceBefore = options.ledger.countEvidence(initial.id);
        const assembled = assembleContext({
            goal: options.ledger.getGoal(initial.id),
            acceptance: options.ledger.listAcceptance(initial.id),
            tasks: options.ledger.listTasks(initial.id),
            dependencies: options.ledger.listDependencies(initial.id),
            task, evidence: options.ledger.listEvidence(initial.id, { limit: 40 }),
            attempts: options.ledger.listAttempts(initial.id, task.id),
            intents: options.ledger.listIntents(initial.id, { taskId: task.id, limit: 30 }),
            repo: before, cycle, modelContextTokens,
            toolSchemaChars: JSON.stringify(options.registry.definitions()).length,
            ...(tokenRatio !== undefined ? { tokenRatio } : {}), strategyIndex,
        });
        goalTodos.refresh(goalTodoSnapshot(options.ledger.listTasks(initial.id), task.id));
        const turnConfig = structuredClone(options.config);
        turnConfig.limits.maxSteps = budgets.maxStepsPerCycle;
        const ctx = {
            cwd: workspace, workspaceRoot: workspace, signal: options.signal,
            todos: goalTodos, config: () => turnConfig,
        };
        const intentByCall = new Map();
        const deniedCalls = new Set();
        let claim = '';
        let cycleOutput = 0;
        let cyclePromptTokens = 0;
        const turnErrors = [];
        let toolBudgetExceeded = false;
        let toolCallsInTurn = 0;
        const onEvent = (event) => {
            options.assertOwnership?.();
            if (event.type === 'text') {
                claim += event.delta;
                emit({ type: 'text', delta: event.delta });
            }
            else if (event.type === 'tool_start') {
                toolCalls++;
                toolCallsInTurn++;
                emit({ type: 'tool_start', step: event.step, name: event.name, args: event.args });
            }
            else if (event.type === 'tool_end') {
                const intentId = intentByCall.get(event.id);
                if (intentId && !deniedCalls.has(event.id)) {
                    options.ledger.finishIntent(intentId, event.outcome.ok ? 'completed' : 'failed', event.outcome.ok, event.outcome.output, event.outcome.error ?? null);
                }
                emit({ type: 'tool_end', name: event.name, ok: event.outcome.ok, output: event.outcome.output || event.outcome.error || '' });
            }
            else if (event.type === 'usage') {
                cycleOutput += Math.max(0, event.evalCount);
                cyclePromptTokens += Math.max(0, event.promptEvalCount);
                emit({ type: 'usage', promptTokens: event.promptEvalCount, outputTokens: event.evalCount });
            }
            else if (event.type === 'error') {
                turnErrors.push(event.message);
                emit({ type: 'error', message: event.message });
            }
        };
        const onToolStart = async (call) => {
            options.assertOwnership?.();
            if (toolCalls >= budgets.maxToolCalls) {
                toolBudgetExceeded = true;
                options.ledger.appendEvent(initial.id, task.id, attempt.id, 'budget.tool_calls', { limit: budgets.maxToolCalls });
                return false;
            }
            const signature = callSignature(call);
            if (blockedMutations.has(signature))
                return false;
            const absolute = toolTarget(call, workspace);
            const intent = options.ledger.recordIntent({
                goalId: initial.id, taskId: task.id, attemptId: attempt.id, callId: call.id,
                name: call.name, args: JSON.stringify(call.args),
                path: absolute ? relative(workspace, absolute) : null,
                beforeHash: absolute ? hashFile(absolute) : null,
            });
            intentByCall.set(call.id, intent.id);
            let allowed = false;
            try {
                allowed = await options.approve(call);
            }
            catch {
                allowed = false;
            }
            options.assertOwnership?.();
            if (!allowed) {
                deniedCalls.add(call.id);
                options.ledger.finishIntent(intent.id, 'denied', false, 'denied by the active permission policy', 'denied');
                options.ledger.appendEvent(initial.id, task.id, attempt.id, 'tool.denied', { signature, name: call.name });
                blockedMutations.add(signature);
            }
            return allowed;
        };
        const prompt = cyclePrompt(assembled.text);
        let result;
        try {
            result = await turn({
                model: options.model, registry: options.registry, config: turnConfig, ctx,
                history: [], userText: prompt, signal: options.signal,
                maxOutputTokens: Math.max(1, budgets.maxOutputTokens - outputTokens),
                blockedMutations, onToolStart, onEvent,
            });
        }
        catch (error) {
            // A lost lease must never result in further goal-ledger writes from the old owner.
            options.assertOwnership?.();
            const message = error instanceof Error ? error.message : String(error);
            const fingerprint = errorFingerprint(message);
            options.ledger.finishAttempt(attempt.id, { status: 'failed', stopReason: 'exception', summary: message, fingerprint });
            options.ledger.addEvidence({
                goalId: initial.id, taskId: task.id, attemptId: attempt.id, kind: 'observation',
                label: 'Supervisor cycle failed', detail: message, exitCode: null, logPath: null,
            });
            options.ledger.updateTask(task.goalId, task.id, { lastError: message, fingerprint, status: 'pending' });
            return stopWith('paused', `cycle failed: ${message}`);
        }
        options.assertOwnership?.();
        outputTokens += cycleOutput;
        options.ledger.bumpGoal(initial.id, { outputTokens: cycleOutput, toolCalls: toolCallsInTurn });
        if (cyclePromptTokens > 0) {
            const requestChars = JSON.stringify([{ role: 'user', content: prompt }]).length;
            if (requestChars > 500)
                tokenRatio = cyclePromptTokens / requestChars;
        }
        const after = snapshot(workspace);
        const completedAfter = options.ledger.listTasks(initial.id).filter(candidate => candidate.status === 'completed').length;
        const evidenceAfter = options.ledger.countEvidence(initial.id);
        const progressed = madeProgress({
            beforeFingerprint: before.fingerprint, afterFingerprint: after.fingerprint,
            completedBefore, completedAfter, evidenceBefore, evidenceAfter,
        });
        const intents = options.ledger.listIntents(initial.id, { attemptId: attempt.id, limit: 200 });
        for (const intent of intents) {
            if (intent.status === 'started') {
                options.ledger.finishIntent(intent.id, 'unknown', null, 'the cycle ended before a result was recorded', 'outcome unknown');
                const signature = intentSignature(intent);
                blockedMutations.add(signature);
                options.ledger.appendEvent(initial.id, task.id, attempt.id, 'tool.uncertain', {
                    signature, name: intent.name, intentId: intent.id,
                });
            }
            if (intent.path && intent.status !== 'denied') {
                const absolute = resolveInside(workspace, intent.path);
                const current = absolute ? hashFile(absolute) : null;
                options.ledger.setTaskFile(initial.id, task.id, {
                    path: intent.path, beforeHash: intent.beforeHash, afterHash: current,
                    changed: current !== intent.beforeHash,
                });
            }
        }
        const finalClaim = claim.trim() || claimOf(result.messages);
        if (finalClaim !== '') {
            options.ledger.addEvidence({
                goalId: initial.id, taskId: task.id, attemptId: attempt.id, kind: 'model_claim',
                label: `Model claim for ${task.id}`, detail: finalClaim.slice(0, 4_000),
                exitCode: null, logPath: null,
            });
        }
        const failures = intents.filter(intent => intent.status === 'failed' || intent.status === 'denied');
        const failureText = [
            failures.length > 0 ? (failures.at(-1).error ?? failures.at(-1).output) : '',
            ...turnErrors,
        ].filter(Boolean).join('\n');
        if (failureText) {
            options.ledger.addEvidence({
                goalId: initial.id, taskId: task.id, attemptId: attempt.id, kind: 'observation',
                label: `Diagnostics for ${task.id}`, detail: failureText.slice(0, 6_000),
                exitCode: null, logPath: null,
            });
        }
        const stop = result.stopReason;
        const summary = finalClaim.slice(0, 2_000) || failureText.slice(0, 2_000) || `stopped: ${stop}`;
        if (stop === 'cancelled') {
            options.ledger.finishAttempt(attempt.id, { status: 'abandoned', stopReason: stop, summary, diffFingerprint: after.fingerprint, progressed });
            options.ledger.updateTask(task.goalId, task.id, { status: 'pending' });
            return stopWith('paused', 'cancelled by user');
        }
        if (stop === 'budget_exhausted' || toolBudgetExceeded) {
            options.ledger.finishAttempt(attempt.id, { status: 'abandoned', stopReason: stop, summary, diffFingerprint: after.fingerprint, progressed });
            options.ledger.updateTask(task.goalId, task.id, { status: 'pending' });
            return stopWith('blocked', stop === 'budget_exhausted'
                ? `goal output-token budget reached (${outputTokens}/${budgets.maxOutputTokens})`
                : `goal tool-call budget reached (${toolCalls}/${budgets.maxToolCalls})`);
        }
        if (stop === 'context_limit' || stop === 'max_steps') {
            options.ledger.finishAttempt(attempt.id, { status: 'abandoned', stopReason: stop, summary, diffFingerprint: after.fingerprint, progressed });
            options.ledger.updateTask(task.goalId, task.id, {
                status: 'in_progress',
                nextAction: 'Continue from the recorded work in a fresh context. Inspect current state before acting; do not replay the previous sequence.',
            });
            options.ledger.saveCheckpoint({ goalId: initial.id, cycle, phase: 'continuation', taskId: task.id, attemptId: attempt.id, note: stop });
            emit({ type: 'checkpoint', cycle, phase: 'continuation', taskId: task.id });
            continue;
        }
        if (stop === 'answered') {
            const verification = await verifyTask(task, attempt.id);
            if (verification.status === 'passed') {
                options.ledger.updateTask(task.goalId, task.id, {
                    status: 'completed', verification: 'passed', evidenceId: verification.evidenceId ?? null,
                    completedAt: new Date().toISOString(), lastError: null, nextAction: null,
                });
                options.ledger.finishAttempt(attempt.id, { status: 'passed', stopReason: stop, summary, diffFingerprint: after.fingerprint, progressed: true });
                options.ledger.saveCheckpoint({ goalId: initial.id, cycle, phase: 'task_complete', taskId: task.id, attemptId: attempt.id, note: task.title });
                emit({ type: 'task', task: options.ledger.getTask(task.id, task.goalId), detail: 'verified complete' });
                emit({ type: 'checkpoint', cycle, phase: 'task_complete', taskId: task.id });
                continue;
            }
            if (verification.status === 'blocked') {
                options.ledger.updateTask(task.goalId, task.id, {
                    status: 'blocked', verification: 'blocked', lastError: verification.detail,
                    nextAction: 'Resolve the permission or environment blocker, then resume this goal.',
                });
                options.ledger.finishAttempt(attempt.id, { status: 'failed', stopReason: stop, summary: verification.detail, fingerprint: errorFingerprint(verification.detail), diffFingerprint: after.fingerprint, progressed });
                emit({ type: 'task', task: options.ledger.getTask(task.id, task.goalId), detail: `verification blocked: ${verification.detail}` });
                continue;
            }
            handleFailure({
                ledger: options.ledger, goalId: initial.id, task: options.ledger.getTask(task.id, task.goalId),
                attempt, stopReason: stop, summary, failure: verification.detail,
                before, after, progressed, emit,
            });
            continue;
        }
        handleFailure({
            ledger: options.ledger, goalId: initial.id, task: options.ledger.getTask(task.id, task.goalId),
            attempt, stopReason: stop, summary, failure: failureText || `turn stopped with ${stop}`,
            before, after, progressed, emit,
        });
        const updated = options.ledger.getTask(task.id, task.goalId);
        if (updated.status === 'blocked')
            emit({ type: 'task', task: updated, detail: `blocked: ${updated.lastError ?? failureText}` });
    }
    return stopWith('blocked', `cycle budget exhausted (${cycle}/${budgets.maxCycles}); progress is preserved`);
}
function handleFailure(input) {
    const fingerprint = errorFingerprint(input.failure);
    const kind = classifyFailure(input.failure);
    const sameFingerprint = input.task.fingerprint === fingerprint;
    const attempts = input.task.attempts + 1;
    const stalled = sameFingerprint && !input.progressed;
    const rotate = stalled || (kind === 'deterministic' && attempts >= 2);
    const nextIndex = rotate ? Math.min(input.task.strategyIndex + 1, RECOVERY_STRATEGIES.length - 1) : input.task.strategyIndex;
    const noStrategyLeft = rotate && nextIndex === input.task.strategyIndex && stalled;
    const exhausted = attempts >= input.task.maxAttempts || noStrategyLeft;
    input.ledger.finishAttempt(input.attempt.id, {
        status: 'failed', stopReason: input.stopReason, summary: input.summary,
        fingerprint, diffFingerprint: input.after.fingerprint, progressed: input.progressed,
    });
    if (kind === 'permission') {
        input.ledger.updateTask(input.task.goalId, input.task.id, {
            status: 'blocked', attempts, lastError: input.failure.slice(0, 4_000), fingerprint,
            nextAction: 'The required action was denied. Do not retry it; an operator must grant permission or revise the task.',
        });
        input.ledger.addEvidence({
            goalId: input.goalId, taskId: input.task.id, attemptId: input.attempt.id,
            kind: 'permission', label: `Permission denied for ${input.task.id}`,
            detail: input.failure.slice(0, 4_000), exitCode: null, logPath: null,
        });
        return;
    }
    if (exhausted) {
        input.ledger.updateTask(input.task.goalId, input.task.id, {
            status: 'blocked', attempts, lastError: input.failure.slice(0, 4_000), fingerprint,
            strategyIndex: nextIndex,
            nextAction: 'Attempt budget exhausted. Preserve the work and report this blocker with the recorded evidence.',
        });
        input.ledger.addEvidence({
            goalId: input.goalId, taskId: input.task.id, attemptId: input.attempt.id,
            kind: 'observation', label: `Task blocked: ${input.task.id}`,
            detail: `${input.failure.slice(0, 6_000)}\n\nAttempts: ${attempts}/${input.task.maxAttempts}. Last strategy: ${strategyName(input.task.strategyIndex)}.`,
            exitCode: null, logPath: null,
        });
        return;
    }
    const strategy = strategyFor(nextIndex);
    input.ledger.updateTask(input.task.goalId, input.task.id, {
        status: 'pending', attempts, lastError: input.failure.slice(0, 4_000), fingerprint,
        strategyIndex: nextIndex, nextAction: strategy.instruction,
    });
    input.emit({
        type: 'recovery', taskId: input.task.id, strategy: strategy.name, fingerprint,
        detail: `${kind}; ${stalled ? 'no measurable progress since the previous attempt' : 'new failure'}`,
    });
    input.ledger.appendEvent(input.goalId, input.task.id, input.attempt.id, 'recovery.selected', {
        strategy: strategy.name, fingerprint, kind, stalled: input.progressed ? false : stalled, progressed: input.progressed,
    });
}
