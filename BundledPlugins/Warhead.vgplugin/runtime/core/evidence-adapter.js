import { exportGoalEvidence } from '../goal/evidence-export.js';
import { ArtifactStore } from '../goal/artifact-store.js';
export async function exportForCoreHandoff(ledger, goalId, artifacts) {
    const pkg = await exportGoalEvidence(ledger, goalId, artifacts);
    return {
        goalId: pkg.goal.id,
        packageSha256: pkg.packageSha256,
        evidenceCount: pkg.evidence.length,
        taskCount: pkg.tasks.length,
        acceptanceCount: pkg.acceptance.length,
    };
}
