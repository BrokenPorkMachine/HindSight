import { createHash } from 'node:crypto';
import { ArtifactIntegrityError, ArtifactStore } from './artifact-store.js';
import { hashFile, repoSnapshot, resolveInside } from './repo.js';
const SECRET_KEY = /(?:token|secret|password|authorization|api.?key|credential)/i;
function redact(value) {
    if (Array.isArray(value))
        return value.map(redact);
    if (typeof value === 'string')
        return value
            .replace(/(\b(?:authorization|api[_ -]?key|access[_ -]?token|secret|password)\b\s*[:=]\s*)([^\s,;]+)/ig, '$1[REDACTED]')
            .replace(/\bBearer\s+[A-Za-z0-9._~+/=-]+/ig, 'Bearer [REDACTED]');
    if (!value || typeof value !== 'object')
        return value;
    return Object.fromEntries(Object.entries(value)
        .filter(([key]) => !SECRET_KEY.test(key))
        .map(([key, item]) => [key, redact(item)]));
}
/** Build a reproducible, redacted projection from the authoritative ledger.
 * Every referenced artifact is re-hashed before it is included. */
export async function exportGoalEvidence(ledger, goalId, artifacts) {
    const goal = ledger.getGoal(goalId);
    if (!goal)
        throw new Error(`goal ${goalId} not found`);
    const staleEvidence = [];
    const currentRevision = repoSnapshot(goal.workspace).commit;
    const evidence = ledger.listEvidence(goalId, { all: true }).map(item => ({
        id: item.id, taskId: item.taskId, attemptId: item.attemptId, kind: item.kind,
        label: redact(item.label), detail: redact(item.detail), exitCode: item.exitCode,
        artifactSha256: item.artifactSha256, workspaceRevision: item.workspaceRevision,
        provenance: redact(item.provenance), createdAt: item.createdAt,
    }));
    const evidenceIds = new Set(evidence.map(item => item.id));
    const acceptance = ledger.listAcceptance(goalId);
    if (goal.status === 'completed') {
        const unprovenCriteria = acceptance.filter(item => item.status !== 'passed' || !item.evidenceId || !evidenceIds.has(item.evidenceId));
        if (unprovenCriteria.length)
            throw new Error(`completed goal has missing or stale mandatory acceptance evidence: ${unprovenCriteria.map(item => item.id).join(', ')}`);
        const unprovenTasks = ledger.listTasks(goalId).filter(item => item.status !== 'completed' || item.verification !== 'passed' || !item.evidenceId || !evidenceIds.has(item.evidenceId));
        if (unprovenTasks.length)
            throw new Error(`completed goal has missing or stale task verification evidence: ${unprovenTasks.map(item => item.id).join(', ')}`);
    }
    for (const criterion of acceptance) {
        if (criterion.status === 'passed' && (!criterion.evidenceId || !evidenceIds.has(criterion.evidenceId))) {
            throw new Error(`acceptance criterion ${criterion.id} has no linked verifier evidence`);
        }
    }
    for (const item of evidence) {
        if (item.artifactSha256 && !(await artifacts.verify(item.artifactSha256))) {
            throw new ArtifactIntegrityError(`evidence artifact missing or corrupt: ${item.artifactSha256}`);
        }
        const checks = item.provenance['fileChecks'];
        if (item.workspaceRevision && item.workspaceRevision !== currentRevision)
            staleEvidence.push(item.id);
        const env = item.provenance['environment'];
        if (env && typeof env === 'object') {
            const recorded = env;
            if ((recorded['node'] && recorded['node'] !== process.version) ||
                (recorded['platform'] && recorded['platform'] !== process.platform) ||
                (recorded['arch'] && recorded['arch'] !== process.arch))
                staleEvidence.push(item.id);
        }
        if (Array.isArray(checks)) {
            for (const raw of checks) {
                if (!raw || typeof raw !== 'object')
                    continue;
                const check = raw;
                if (typeof check.path !== 'string' || typeof check.exists !== 'boolean')
                    continue;
                const absolute = resolveInside(goal.workspace, check.path);
                const current = absolute ? hashFile(absolute) : null;
                const currentExists = current !== null;
                if (currentExists !== check.exists || current !== (typeof check.sha256 === 'string' ? check.sha256 : null)) {
                    staleEvidence.push(item.id);
                }
            }
        }
    }
    if (staleEvidence.length)
        throw new Error(`stale evidence references changed workspace inputs: ${[...new Set(staleEvidence)].join(', ')}`);
    const artifactDigests = [...new Set(evidence.flatMap(item => item.artifactSha256 ? [item.artifactSha256] : []))].sort();
    const artifactBytes = {};
    for (const digest of artifactDigests)
        artifactBytes[digest] = (await artifacts.readVerified(digest)).toString('base64');
    const payload = {
        schemaVersion: 1,
        goal: { id: goal.id, objective: redact(goal.objective), status: goal.status, branch: goal.branch, commit: goal.commit },
        tasks: ledger.listTasks(goalId).map(task => ({
            id: task.id, title: redact(task.title), status: task.status, verification: task.verification,
            verifier: redact(task.verifier),
        })),
        acceptance: acceptance.map(item => redact(item)),
        evidence,
        artifacts: artifactBytes,
    };
    const canonical = JSON.stringify(payload);
    return { ...payload, packageSha256: createHash('sha256').update(canonical).digest('hex') };
}
