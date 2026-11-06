/**
 * Dreaming (self-improvement) contracts.
 *
 * Status: Proposed per docs/architecture/DREAMING_SELF_IMPROVEMENT_SPEC.md.
 * No background worker or self-update is enabled by default.
 */
export const DREAMING_CONTROL_PLANE_SENSITIVE_PATHS = [
    '.github/workflows',
    'CODEOWNERS',
    'docs/architecture/VERIFICATION_CONTRACT.md',
    'docs/architecture/VERIFIED_AUTONOMOUS_ENGINEERING_SPEC.md',
    'src/perm',
    'src/goal/supervisor.ts',
    'src/goal/verify.ts',
    'src/goal/ledger.ts',
    'core/contracts',
    'core/service',
];
export const DREAMING_BRANCH_PREFIX = 'dream/';
export const DREAMING_TASK_BRANCH_PREFIX = 'dream/task/';
