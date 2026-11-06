/**
 * Durable goal-execution vocabulary.
 *
 * A goal, a task and a model turn are three different lifetimes. These records
 * describe the first two; `runTurn` owns the third. Nothing here is derived
 * from a conversation: the ledger is authoritative and a model context is a
 * disposable execution resource assembled from it.
 */
export const DEFAULT_GOAL_BUDGETS = {
    maxCycles: 30,
    maxOutputTokens: 400_000,
    maxDurationMs: 2 * 60 * 60 * 1000,
    maxTaskAttempts: 3,
    stagnationLimit: 2,
    maxRemediationTasks: 3,
    maxToolCalls: 600,
    maxStepsPerCycle: 80,
    maxPlanTasks: 24,
};
