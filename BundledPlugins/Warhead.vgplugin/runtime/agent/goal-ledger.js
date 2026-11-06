/**
 * Durable, single-host goal ledger for supervised Warhead coding tasks.
 * The model's todo tool is a projection; it cannot mark work verified.
 *
 * Single-writer lock is fail-closed. A stale lock must be investigated, never
 * silently stolen: another host/process may be performing a mutation.
 */
import { randomUUID } from 'node:crypto';
import { closeSync, existsSync, fsyncSync, mkdirSync, openSync, readFileSync, renameSync, unlinkSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
const fresh = () => ({ schemaVersion: 1, revision: 0, goals: [] });
const clone = (x) => structuredClone(x);
const now = () => new Date().toISOString();
const good = (value, field, max = 300) => {
    if (typeof value !== 'string' || !value.trim() || value.length > max || /[\u0000-\u0008\u000b\u000c\u000e-\u001f]/.test(value))
        throw Error('invalid ' + field);
    return value.trim();
};
const validId = (id) => typeof id === 'string' && /^[a-f0-9-]{36}$/.test(id);
const taskIn = (goal, id) => {
    const task = goal.tasks.find(t => t.id === id);
    if (!task)
        throw Error('task not found');
    return task;
};
function validateGraph(goal) {
    const map = new Map(goal.tasks.map(t => [t.id, t]));
    if (map.size !== goal.tasks.length)
        throw Error('duplicate task identifiers');
    const active = new Set(), visited = new Set();
    const visit = (id) => {
        if (active.has(id))
            throw Error('cyclic task dependencies');
        if (visited.has(id))
            return;
        const task = map.get(id);
        if (!task)
            throw Error('unknown task dependency');
        active.add(id);
        for (const dependency of task.dependencies) {
            if (dependency === id || !map.has(dependency))
                throw Error('invalid task dependency');
            visit(dependency);
        }
        active.delete(id);
        visited.add(id);
    };
    for (const id of map.keys())
        visit(id);
}
function parse(raw) {
    const state = JSON.parse(raw);
    if (!state || state.schemaVersion !== 1 || !Number.isSafeInteger(state.revision) || state.revision < 0 || !Array.isArray(state.goals) || state.goals.length > 100)
        throw Error('invalid goal ledger');
    const ids = new Set();
    for (const goal of state.goals) {
        if (!goal || !validId(goal.id) || ids.has(goal.id) || typeof goal.title !== 'string' || typeof goal.owner !== 'string' ||
            !Number.isSafeInteger(goal.revision) || !Array.isArray(goal.tasks) || goal.tasks.length > 250 ||
            !Array.isArray(goal.evidence) || goal.evidence.length > 1000)
            throw Error('invalid goal record');
        ids.add(goal.id);
        for (const task of goal.tasks) {
            if (!task || !validId(task.id) || typeof task.title !== 'string' || !['pending', 'in_progress', 'blocked', 'verified'].includes(task.status) ||
                !Array.isArray(task.dependencies) || !task.dependencies.every(validId) || !Array.isArray(task.evidenceIds))
                throw Error('invalid task record');
        }
        for (const evidence of goal.evidence) {
            if (!evidence || !validId(evidence.id) || !validId(evidence.taskId) ||
                !['test', 'build', 'inspection', 'artifact'].includes(evidence.kind) ||
                !['passed', 'failed'].includes(evidence.outcome) ||
                typeof evidence.description !== 'string' || typeof evidence.observedBy !== 'string' ||
                typeof evidence.reference !== 'string')
                throw Error('invalid evidence record');
        }
        validateGraph(goal);
        const evidenceMap = new Map(goal.evidence.map(e => [e.id, e]));
        if (evidenceMap.size !== goal.evidence.length)
            throw Error('duplicate evidence identifiers');
        for (const e of goal.evidence)
            taskIn(goal, e.taskId);
        for (const task of goal.tasks) {
            if (new Set(task.evidenceIds).size !== task.evidenceIds.length ||
                task.evidenceIds.some(id => evidenceMap.get(id)?.taskId !== task.id))
                throw Error('invalid task evidence references');
            if (task.status === 'verified' &&
                (!task.evidenceIds.some(id => evidenceMap.get(id)?.outcome === 'passed') ||
                    task.dependencies.some(id => taskIn(goal, id).status !== 'verified')))
                throw Error('unverified task marked verified');
        }
    }
    return state;
}
export function openGoalLedger(filename) {
    const file = resolve(filename), directory = dirname(file), lock = file + '.lock';
    function load() {
        return existsSync(file) ? parse(readFileSync(file, 'utf8')) : fresh();
    }
    function transact(operation, expectedRevision) {
        mkdirSync(directory, { recursive: true, mode: 0o700 });
        let fd;
        try {
            fd = openSync(lock, 'wx', 0o600);
        }
        catch (error) {
            throw Error('goal ledger locked; investigate another writer before retrying', { cause: error });
        }
        let temp;
        try {
            const state = load();
            if (expectedRevision !== undefined && state.revision !== expectedRevision)
                throw Error('goal ledger revision conflict');
            const result = operation(state);
            state.revision++;
            // Revalidate the full graph before publishing a snapshot.
            parse(JSON.stringify(state));
            temp = join(directory, '.' + basename(file) + '.' + randomUUID() + '.tmp');
            const output = openSync(temp, 'wx', 0o600);
            try {
                writeFileSync(output, JSON.stringify(state, null, 2) + '\n');
                fsyncSync(output);
            }
            finally {
                closeSync(output);
            }
            renameSync(temp, file);
            temp = undefined;
            // Directory fsync is best-effort on filesystems that do not support it.
            try {
                const dirfd = openSync(directory, 'r');
                try {
                    fsyncSync(dirfd);
                }
                finally {
                    closeSync(dirfd);
                }
            }
            catch { }
            return clone(result);
        }
        finally {
            if (temp) {
                try {
                    unlinkSync(temp);
                }
                catch { }
            }
            closeSync(fd);
            unlinkSync(lock);
        }
    }
    const goal = (state, id) => {
        const found = state.goals.find(g => g.id === id);
        if (!found)
            throw Error('goal not found');
        return found;
    };
    const touch = (g) => { g.revision++; g.updatedAt = now(); };
    return {
        snapshot: () => clone(load()),
        createGoal(title, owner, expectedRevision) {
            const name = good(title, 'goal title'), who = good(owner, 'owner', 120);
            return transact(state => {
                if (state.goals.length >= 100)
                    throw Error('goal limit reached');
                const created = now(), g = { id: randomUUID(), title: name, owner: who, revision: 0, createdAt: created, updatedAt: created, tasks: [], evidence: [] };
                state.goals.push(g);
                return g;
            }, expectedRevision);
        },
        addTask(goalId, title, dependencies = [], expectedRevision) {
            const name = good(title, 'task title', 1000);
            return transact(state => {
                const g = goal(state, goalId);
                if (g.tasks.length >= 250)
                    throw Error('task limit reached');
                if (!Array.isArray(dependencies) || new Set(dependencies).size !== dependencies.length ||
                    dependencies.some(id => !g.tasks.some(t => t.id === id)))
                    throw Error('invalid task dependencies');
                const at = now(), t = { id: randomUUID(), title: name, dependencies: [...dependencies], status: 'pending', createdAt: at, updatedAt: at, evidenceIds: [] };
                g.tasks.push(t);
                touch(g);
                return t;
            }, expectedRevision);
        },
        setDependencies(goalId, taskId, dependencies, expectedRevision) {
            return transact(state => {
                const g = goal(state, goalId), t = taskIn(g, taskId);
                if (t.status === 'verified')
                    throw Error('verified task dependencies are immutable');
                if (!Array.isArray(dependencies) || new Set(dependencies).size !== dependencies.length ||
                    dependencies.some(id => !g.tasks.some(x => x.id === id)))
                    throw Error('invalid task dependencies');
                t.dependencies = [...dependencies];
                validateGraph(g);
                t.updatedAt = now();
                touch(g);
                return t;
            }, expectedRevision);
        },
        setTaskStatus(goalId, taskId, status, reason, expectedRevision) {
            if (!['pending', 'in_progress', 'blocked'].includes(status))
                throw Error('verified status requires evidence and supervisor verification');
            return transact(state => {
                const g = goal(state, goalId), t = taskIn(g, taskId);
                if (t.status === 'verified')
                    throw Error('verified task cannot be reopened without a new task');
                if (status === 'in_progress' && t.dependencies.some(id => taskIn(g, id).status !== 'verified'))
                    throw Error('dependencies not verified');
                if (status === 'blocked')
                    t.blockedReason = good(reason, 'blocker', 2000);
                else
                    delete t.blockedReason;
                t.status = status;
                t.updatedAt = now();
                touch(g);
                return t;
            }, expectedRevision);
        },
        // Trusted supervisory caller only: never expose as a model tool.
        recordEvidence(goalId, taskId, input, expectedRevision) {
            if (!['test', 'build', 'inspection', 'artifact'].includes(input.kind) || !['passed', 'failed'].includes(input.outcome))
                throw Error('invalid evidence');
            const description = good(input.description, 'evidence description', 1000);
            const observedBy = good(input.observedBy, 'evidence observer', 120);
            const reference = good(input.reference, 'evidence reference', 2000);
            return transact(state => {
                const g = goal(state, goalId), t = taskIn(g, taskId);
                if (t.status === 'verified')
                    throw Error('verified task evidence is immutable');
                if (g.evidence.length >= 1000)
                    throw Error('evidence limit reached');
                const e = { id: randomUUID(), taskId, kind: input.kind, description, outcome: input.outcome, observedBy, reference, observedAt: now() };
                g.evidence.push(e);
                t.evidenceIds.push(e.id);
                t.updatedAt = now();
                touch(g);
                return e;
            }, expectedRevision);
        },
        // Verification is a separate supervisor-authorized action, not a todo update.
        verifyTask(goalId, taskId, evidenceIds, expectedRevision) {
            return transact(state => {
                const g = goal(state, goalId), t = taskIn(g, taskId);
                if (t.status === 'verified')
                    throw Error('task already verified');
                if (t.dependencies.some(id => taskIn(g, id).status !== 'verified'))
                    throw Error('dependencies not verified');
                if (!Array.isArray(evidenceIds) || !evidenceIds.length || new Set(evidenceIds).size !== evidenceIds.length)
                    throw Error('passing evidence required');
                const selected = evidenceIds.map(id => g.evidence.find(e => e.id === id && e.taskId === taskId));
                if (selected.some(e => !e || e.outcome !== 'passed'))
                    throw Error('passing evidence required');
                // A later failure must be addressed; old passing evidence cannot mask it.
                const latest = g.evidence.filter(e => e.taskId === taskId).at(-1);
                if (latest?.outcome !== 'passed')
                    throw Error('latest task observation did not pass');
                t.status = 'verified';
                t.verifiedAt = now();
                t.updatedAt = t.verifiedAt;
                delete t.blockedReason;
                touch(g);
                return t;
            }, expectedRevision);
        },
        pendingTasks(goalId) {
            const g = goal(load(), goalId);
            return clone(g.tasks.filter(t => t.status !== 'verified' && t.dependencies.every(id => taskIn(g, id).status === 'verified')));
        },
    };
}
