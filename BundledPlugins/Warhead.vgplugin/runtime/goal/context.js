/**
 * Task-aware context assembly.
 *
 * The conversation is disposable. This module rebuilds the working context from
 * the ledger on every cycle, in priority order:
 *
 *   1. permanent goal + constraints
 *   2. current task, its acceptance checks and recovery state
 *   3. goal acceptance and plan progress
 *   4. verified evidence and diagnostics
 *   5. repository state
 *   6. bounded recent observations
 *   7. retrieved historical evidence
 *
 * Lower layers are dropped before higher ones. The objective is never dropped.
 */
import { inputBudgetChars } from '../context/budget.js';
import { strategyFor } from './recovery.js';
const CAPS = {
    goal: 6_000,
    task: 8_000,
    acceptance: 5_000,
    plan: 4_500,
    evidence: 8_000,
    recovery: 4_500,
    repo: 3_500,
    observations: 6_000,
    history: 4_000,
};
function clip(text, max) {
    if (text.length <= max)
        return { text, dropped: false };
    const suffix = '\n… [truncated to fit the context budget]';
    return { text: text.slice(0, Math.max(0, max - suffix.length)) + suffix, dropped: true };
}
function taskLine(task) {
    const mark = task.status === 'completed' ? 'x'
        : task.status === 'in_progress' || task.status === 'verification' ? '~'
            : task.status === 'blocked' || task.status === 'failed' ? '!'
                : task.status === 'ready' ? '>' : ' ';
    return `[${mark}] ${task.id} ${task.title} (${task.status}${task.verification !== 'pending' ? `, verification ${task.verification}` : ''})`;
}
function renderGoal(input) {
    const goal = input.goal;
    return [
        '# Permanent goal',
        `Objective: ${goal.objective}`,
        `Goal id: ${goal.id}`,
        `Workspace: ${goal.workspace}`,
        `Cycle: ${input.cycle} of ${goal.budgets.maxCycles}`,
        '',
        'Constraints:',
        '- The ledger is authoritative. Do not restate or overwrite completed work.',
        '- A model statement that something is done is a claim, not proof. The supervisor runs verification separately.',
        '- Respect the permission policy. If an action is denied, do not retry the same action.',
        '- Never repeat an identical failing action. If the same error recurs, change approach or record a blocker.',
        '- Preserve the objective even if this context is replaced.',
    ].join('\n');
}
function renderTask(input) {
    const task = input.task;
    const strategy = strategyFor(input.strategyIndex ?? task.strategyIndex);
    const dependencies = input.dependencies.filter(dependency => dependency.taskId === task.id).map(dependency => dependency.dependsOn);
    const verifier = task.verifier;
    return [
        '# Current task',
        `Task: ${task.id} — ${task.title}`,
        `Status: ${task.status}`,
        `Description: ${task.description || '(none)'}`,
        `Depends on: ${dependencies.length > 0 ? dependencies.join(', ') : '(none)'}`,
        `Attempt: ${task.attempts + 1} of ${task.maxAttempts}`,
        `Verification policy: ${task.policy}`,
        verifier.commands.length > 0 ? `Verification commands:\n${verifier.commands.map(command => `  $ ${command}`).join('\n')}` : 'Verification commands: none configured',
        verifier.files.length > 0 ? `Required files: ${verifier.files.join(', ')}` : '',
        verifier.checks.length > 0 ? `Acceptance checks:\n${verifier.checks.map(check => `  - ${check}`).join('\n')}` : '',
        '',
        `# Recovery strategy: ${strategy.name}`,
        strategy.instruction,
        task.nextAction ? `Next action from the previous attempt: ${task.nextAction}` : '',
        task.lastError ? `Last failure (${task.fingerprint ?? 'unfingerprinted'}): ${task.lastError.slice(0, 1_500)}` : '',
        '',
        '# This cycle',
        'Work only on the current task. You have a bounded number of tool steps.',
        'When the task is implemented, stop calling tools and state exactly what you changed and which command should prove it.',
        'Do not mark the goal complete. The supervisor decides completion from evidence.',
    ].filter(Boolean).join('\n');
}
function renderAcceptance(input) {
    return [
        '# Goal acceptance criteria (checked by the supervisor)',
        ...input.acceptance.map(criterion => `- [${criterion.status}] ${criterion.description}${criterion.command ? `\n    $ ${criterion.command}` : ''}`),
    ].join('\n');
}
function renderPlan(input) {
    const completed = input.tasks.filter(task => task.status === 'completed').length;
    return [
        `# Plan progress (${completed}/${input.tasks.length} verified)`,
        ...input.tasks.map(taskLine),
    ].join('\n');
}
function renderEvidence(input) {
    const recent = input.evidence.slice(0, 12);
    if (recent.length === 0)
        return '# Evidence\nNo evidence recorded yet.';
    const lines = recent.map(evidence => {
        const exit = evidence.exitCode === null ? '' : ` exit=${evidence.exitCode}`;
        return `- [${evidence.kind}] ${evidence.label}${exit}${evidence.logPath ? ` (log: ${evidence.logPath})` : ''}\n  ${evidence.detail.replace(/\n/g, '\n  ').slice(0, 700)}`;
    });
    return `# Evidence (newest first)\n${lines.join('\n')}`;
}
function renderRepo(input) {
    const repo = input.repo;
    return [
        '# Repository state',
        repo.isRepo ? `Branch: ${repo.branch ?? '(detached)'}\nCommit: ${repo.commit ?? '(none)'}` : 'Not a git repository.',
        `Working tree: ${repo.dirty ? 'modified' : 'clean'}`,
        repo.statusText ? `Changed paths:\n${repo.statusText.split('\n').slice(0, 40).join('\n')}` : '',
    ].filter(Boolean).join('\n');
}
function renderObservations(input) {
    const intentLines = input.intents.slice(0, 14).reverse().map(intent => {
        const args = intent.args.length > 220 ? `${intent.args.slice(0, 220)}…` : intent.args;
        const outcome = intent.status === 'completed' ? `ok=${intent.ok ?? '?'}` : intent.status;
        const output = intent.output.replace(/\s+/g, ' ').slice(0, 240);
        return `- ${intent.name}(${args}) -> ${outcome}${output ? `: ${output}` : ''}`;
    });
    const attemptLines = input.attempts.slice(-4).map(attempt => `- attempt ${attempt.cycle} strategy=${attempt.strategy} status=${attempt.status} stop=${attempt.stopReason ?? '?'} ${attempt.summary.slice(0, 240)}`);
    return [
        '# Recent observations (bounded)',
        attemptLines.length > 0 ? `Attempts:\n${attemptLines.join('\n')}` : '',
        intentLines.length > 0 ? `Tools:\n${intentLines.join('\n')}` : '',
        attemptLines.length === 0 && intentLines.length === 0 ? 'None yet. Inspect before acting.' : '',
    ].filter(Boolean).join('\n');
}
function renderHistory(input) {
    const related = input.evidence
        .filter(evidence => evidence.taskId === input.task.id)
        .slice(0, 8);
    if (related.length === 0)
        return '# Retrieved history for this task\nNo earlier evidence for this task.';
    return `# Retrieved history for this task\n${related.map(evidence => `- [${evidence.kind}] ${evidence.label}: ${evidence.detail.replace(/\s+/g, ' ').slice(0, 500)}`).join('\n')}`;
}
export function assembleContext(input) {
    const advertised = input.modelContextTokens;
    const estimatedFiles = input.toolSchemaChars;
    const budgetChars = Math.max(8_000, Math.min(inputBudgetChars(advertised) - estimatedFiles - 6_000, 
    // A conservative cap even on a very large context: supervisor cycles are
    // cheaper to repeat than a single request that overflows the daemon.
    140_000));
    // tokenRatio is tokens per char. Using it converts known window tokens into a
    // char allowance without trusting the 4-chars-per-token approximation.
    const ratio = input.tokenRatio && input.tokenRatio > 0 ? input.tokenRatio : 0.25;
    const adaptive = input.modelContextTokens ? Math.floor((input.modelContextTokens * 0.75) / ratio) : budgetChars;
    const effective = Math.max(8_000, Math.min(budgetChars, adaptive));
    const sections = [
        { name: 'goal', text: renderGoal(input) },
        { name: 'task', text: renderTask(input) },
        { name: 'acceptance', text: renderAcceptance(input) },
        { name: 'plan', text: renderPlan(input) },
        { name: 'evidence', text: renderEvidence(input) },
        { name: 'repo', text: renderRepo(input) },
        { name: 'observations', text: renderObservations(input) },
        { name: 'history', text: renderHistory(input) },
    ];
    let remaining = effective;
    const layers = [];
    const rendered = [];
    for (const section of sections) {
        const cap = Math.min(CAPS[section.name], Math.max(0, remaining));
        const clipped = clip(section.text, cap);
        if (cap <= 0) {
            layers.push({ name: section.name, chars: 0, cap: CAPS[section.name], dropped: true });
            continue;
        }
        rendered.push(clipped.text);
        layers.push({ name: section.name, chars: clipped.text.length, cap: CAPS[section.name], dropped: clipped.dropped });
        remaining -= clipped.text.length + 2;
    }
    return { text: rendered.join('\n\n'), layers, budgetChars: effective };
}
/** The task prompt for one bounded cycle. Kept separate from the system prompt,
 *  which still comes from the model's Modelfile and the shared agent prompt. */
export function cyclePrompt(context) {
    return [
        'You are executing one bounded cycle of a persistent development goal.',
        'The structured state below is authoritative and was loaded from a durable ledger, not from chat history.',
        '',
        context,
    ].join('\n');
}
