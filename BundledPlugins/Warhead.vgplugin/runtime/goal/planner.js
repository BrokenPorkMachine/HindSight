/**
 * Autonomous planner.
 *
 * The model proposes a plan; this module decides whether the proposal is a
 * valid plan. Invalid dependency graphs, duplicate ids, empty tasks and
 * unbounded decomposition are rejected before anything reaches the ledger.
 * When a model cannot produce valid JSON, a conservative heuristic plan is used
 * and the fallback is recorded rather than pretending the model planned it.
 */
import { chatOnce } from '../model/client.js';
import { requestContextLength } from '../model/modelfile.js';
const MAX_TITLE = 160;
const MAX_TEXT = 2_000;
const MAX_COMMAND = 400;
const MAX_DEPENDENCIES = 16;
const PLANNER_SYSTEM = `You are the planning stage of an autonomous software-engineering agent.
Translate the objective into a small dependency-aware task plan.

Return ONLY a JSON object with this shape:
{
  "tasks": [
    {
      "id": "T1",
      "title": "short imperative task",
      "description": "what to change and how to know it is done",
      "milestone": "optional milestone name",
      "dependsOn": ["T1"],
      "verification": {
        "commands": ["exact shell command that proves this task"],
        "files": ["optional relative path that must exist"],
        "checks": ["human-readable acceptance checks"]
      }
    }
  ],
  "acceptance": [
    { "description": "objective-level criterion", "command": "optional shell command" }
  ]
}

Rules:
- Produce between 2 and the stated maximum number of tasks. Never exceed it.
- Tasks must be independently verifiable and ordered by dependency.
- Prefer the repository's real build/test commands from the supplied manifest.
- Include at least one acceptance criterion for the overall objective.
- Do not claim anything has already been done. This is a plan, not a status report.
- Do not include prose outside the JSON object.`;
function bounded(value, max) {
    return value.trim().slice(0, max);
}
function asString(value) {
    return typeof value === 'string' && value.trim() !== '' ? value.trim() : null;
}
function asStringArray(value, max, each) {
    if (!Array.isArray(value))
        return [];
    return [...new Set(value
            .filter((entry) => typeof entry === 'string')
            .map(entry => entry.trim())
            .filter(entry => entry !== '' && entry.length <= each))].slice(0, max);
}
function parseJsonObject(text) {
    const trimmed = text.trim();
    const unfenced = trimmed.replace(/^```(?:json)?\s*/i, '').replace(/```$/, '').trim();
    try {
        return JSON.parse(unfenced);
    }
    catch {
        const start = unfenced.indexOf('{');
        const end = unfenced.lastIndexOf('}');
        if (start >= 0 && end > start) {
            try {
                return JSON.parse(unfenced.slice(start, end + 1));
            }
            catch {
                return null;
            }
        }
        return null;
    }
}
export class PlanValidationError extends Error {
}
/** Validate and normalise a candidate plan. Throws PlanValidationError. */
export function normalizePlan(candidate, maxTasks, hints) {
    if (!candidate || typeof candidate !== 'object' || Array.isArray(candidate))
        throw new PlanValidationError('plan is not an object');
    const root = candidate;
    const rawTasks = root['tasks'];
    if (!Array.isArray(rawTasks) || rawTasks.length === 0)
        throw new PlanValidationError('plan has no tasks');
    if (rawTasks.length > maxTasks)
        throw new PlanValidationError(`plan has ${rawTasks.length} tasks; the limit is ${maxTasks}`);
    const idMap = new Map();
    const tasks = [];
    for (const [index, entry] of rawTasks.entries()) {
        if (!entry || typeof entry !== 'object' || Array.isArray(entry))
            throw new PlanValidationError(`task ${index + 1} is not an object`);
        const raw = entry;
        const title = asString(raw['title']);
        if (!title)
            throw new PlanValidationError(`task ${index + 1} has no title`);
        const originalId = asString(raw['id']) ?? `T${index + 1}`;
        const canonical = `T${String(index + 1).padStart(3, '0')}`;
        if (idMap.has(originalId.toLowerCase()))
            throw new PlanValidationError(`duplicate task id '${originalId}'`);
        idMap.set(originalId.toLowerCase(), canonical);
        const verification = raw['verification'] && typeof raw['verification'] === 'object' && !Array.isArray(raw['verification'])
            ? raw['verification'] : {};
        const task = {
            id: canonical,
            title: bounded(title, MAX_TITLE),
            description: bounded(asString(raw['description']) ?? title, MAX_TEXT),
            milestone: asString(raw['milestone']) ? bounded(asString(raw['milestone']), 120) : undefined,
            dependsOn: asStringArray(raw['dependsOn'], MAX_DEPENDENCIES, 80),
            verification: {
                commands: asStringArray(verification['commands'], 4, MAX_COMMAND),
                files: asStringArray(verification['files'], 8, 200),
                checks: asStringArray(verification['checks'], 8, 300),
            },
            _originalId: originalId,
        };
        tasks.push(task);
    }
    const known = new Map();
    for (const task of tasks)
        known.set(task._originalId.toLowerCase(), task);
    for (const task of tasks) {
        const resolved = [];
        for (const dependency of task.dependsOn) {
            const match = known.get(dependency.toLowerCase());
            if (!match)
                throw new PlanValidationError(`task '${task.title}' depends on unknown task '${dependency}'`);
            if (match === task)
                throw new PlanValidationError(`task '${task.title}' depends on itself`);
            resolved.push(match.id);
        }
        task.dependsOn = [...new Set(resolved)];
    }
    // Reject cycles rather than silently dropping edges.
    const byId = new Map(tasks.map(task => [task.id, task]));
    const state = new Map();
    const visit = (taskId) => {
        const current = state.get(taskId) ?? 0;
        if (current === 1)
            throw new PlanValidationError(`dependency cycle at ${taskId}`);
        if (current === 2)
            return;
        state.set(taskId, 1);
        for (const dependency of byId.get(taskId)?.dependsOn ?? [])
            visit(dependency);
        state.set(taskId, 2);
    };
    for (const task of tasks)
        visit(task.id);
    for (const task of tasks) {
        delete task._originalId;
        const commands = task.verification?.commands ?? [];
        const requested = task.verification?.policy;
        const policy = commands.length > 0 ? 'command' : requestPolicy(requested);
        task.verification = { ...task.verification, policy, commands, files: task.verification?.files ?? [], checks: task.verification?.checks ?? [] };
        task.milestone = task.milestone ?? 'goal';
    }
    const acceptance = [];
    if (Array.isArray(root['acceptance'])) {
        for (const entry of root['acceptance'].slice(0, 8)) {
            if (!entry || typeof entry !== 'object' || Array.isArray(entry))
                continue;
            const raw = entry;
            const description = asString(raw['description']);
            if (!description)
                continue;
            const command = asString(raw['command']);
            acceptance.push({
                description: bounded(description, MAX_TEXT),
                ...(command ? { command: bounded(command, MAX_COMMAND) } : {}),
                ...(Number.isSafeInteger(raw['expectedExit']) ? { expectedExit: Number(raw['expectedExit']) } : {}),
                ...(Number.isSafeInteger(raw['timeoutMs']) ? { timeoutMs: Number(raw['timeoutMs']) } : {}),
            });
        }
    }
    acceptance.push(...defaultAcceptance(tasks, hints));
    return { objective: '', tasks, acceptance: dedupeAcceptance(acceptance) };
}
function requestPolicy(value) {
    return value === 'command' || value === 'evidence' ? value : 'evidence';
}
function defaultAcceptance(tasks, hints) {
    const criteria = [{ description: 'Every plan task is completed with recorded verification evidence' }];
    for (const hint of hints.slice(0, 3))
        criteria.push({ description: hint.description, command: hint.command });
    const missing = tasks.filter(task => (task.verification?.commands ?? []).length === 0);
    if (missing.length === 0 && tasks.length > 0) {
        criteria.push({ description: 'Every task-level verification command passes' });
    }
    return criteria;
}
function dedupeAcceptance(criteria) {
    const seen = new Set();
    const out = [];
    for (const criterion of criteria) {
        const key = `${criterion.description}\0${criterion.command ?? ''}`;
        if (seen.has(key))
            continue;
        seen.add(key);
        out.push(criterion);
        if (out.length >= 10)
            break;
    }
    return out;
}
/** A fallback that is deliberately modest: inspect, implement, verify. It does
 *  not invent repository-specific details it never observed. */
export function heuristicPlan(input) {
    const commands = input.hints.map(hint => hint.command);
    const files = input.overview.manifests.slice(0, 4);
    const tasks = [
        {
            id: 'T001',
            milestone: 'orient',
            title: 'Inspect the repository and establish current state',
            description: 'Read the relevant architecture, manifests and existing implementation. Record what exists, what is missing, and the commands that can verify the objective. Do not modify files in this task.',
            dependsOn: [],
            verification: { policy: 'evidence', commands: [], files: [], checks: ['At least one concrete repository observation is recorded'] },
        },
        {
            id: 'T002',
            milestone: 'execute',
            title: `Implement the objective: ${bounded(input.objective, 120)}`,
            description: 'Make the smallest coherent set of changes that satisfies the objective. Read before editing, keep a record of files changed, and do not claim completion until the verification task has run.',
            dependsOn: ['T001'],
            verification: { policy: 'evidence', commands: [], files, checks: ['Files required by the objective exist and the working tree contains the intended change'] },
        },
        {
            id: 'T003',
            milestone: 'verify',
            title: commands.length > 0 ? 'Build and test the result' : 'Inspect the diff and record verification limits',
            description: commands.length > 0
                ? 'Run the repository build and test commands. Fix failures with a changed approach rather than repeating an identical edit. Record exit codes and full logs as evidence.'
                : 'No build or test command was detected. Inspect the diff, run whatever advisory checks are available, and explicitly record what could not be verified.',
            dependsOn: ['T002'],
            verification: {
                policy: commands.length > 0 ? 'command' : 'evidence',
                commands,
                files: [],
                checks: commands.length > 0 ? ['All discovered build/test commands exit successfully'] : ['The diff was inspected and unverified claims are stated explicitly'],
            },
        },
    ];
    const acceptance = [{ description: 'Every plan task is completed with recorded verification evidence' }];
    for (const hint of input.hints.slice(0, 3))
        acceptance.push({ description: hint.description, command: hint.command });
    if (commands.length === 0)
        acceptance.push({ description: 'The final diff was inspected and no automated verification is claimed' });
    return { objective: input.objective, tasks, acceptance: dedupeAcceptance(acceptance) };
}
function renderInventory(input) {
    return JSON.stringify({
        objective: input.objective,
        repository: {
            branch: input.repo.branch,
            commit: input.repo.commit,
            dirty: input.repo.dirty,
            status: input.repo.statusText.slice(0, 4_000),
        },
        manifests: input.overview.manifests,
        scripts: input.overview.scripts,
        files: input.overview.files.slice(0, 120),
        observedVerificationCommands: input.hints,
        maximumTasks: input.maxTasks,
    }, null, 2);
}
export async function createPlan(input) {
    const heuristic = () => ({
        plan: { ...heuristicPlan(input), fallbackReason: 'model plan was unavailable or invalid' },
        source: 'heuristic',
    });
    if (input.signal.aborted)
        throw new Error('planning cancelled');
    const complete = input.complete ?? chatOnce;
    const messages = [
        { role: 'system', content: PLANNER_SYSTEM },
        { role: 'user', content: renderInventory(input) },
    ];
    try {
        const response = await complete(input.model, messages, {
            signal: input.signal, numPredict: 1_600, format: 'json', think: false,
            numCtx: requestContextLength(null, JSON.stringify(messages).length),
        });
        const parsed = parseJsonObject(response.content);
        if (parsed === null)
            return heuristic();
        const plan = normalizePlan(parsed, input.maxTasks, input.hints);
        return { plan: { ...plan, objective: input.objective }, source: 'model' };
    }
    catch (error) {
        if (input.signal.aborted)
            throw error;
        const result = heuristic();
        result.plan.fallbackReason = `model plan rejected: ${error instanceof Error ? error.message : String(error)}`;
        return result;
    }
}
