/**
 * Dreaming worktree utilities (D2 stub).
 * Isolated worktrees for task-scoped development; no protected-branch writes.
 */
import { DREAMING_TASK_BRANCH_PREFIX } from './types.js';
export function taskBranchName(taskId) {
    // Ensure we don't create protected branches
    return `${DREAMING_TASK_BRANCH_PREFIX}${taskId}`;
}
export class DreamWorktreeManager {
    enabled = false;
    isEnabled() { return this.enabled; }
    async create(_spec) {
        if (!this.enabled)
            return { ok: false, reason: 'dreaming development not enabled' };
        throw new Error('not implemented');
    }
    async cleanup(_taskId) {
        // no-op by default
    }
}
