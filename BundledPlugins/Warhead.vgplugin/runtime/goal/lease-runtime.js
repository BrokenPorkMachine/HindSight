/** A single local process owns a goal until release or explicit operator recovery. */
import { hostname } from 'node:os';
import { randomUUID } from 'node:crypto';
export const GOAL_LEASE_TTL_MS = 30_000;
export const GOAL_LEASE_HEARTBEAT_MS = 5_000;
/** Ownership is checked synchronously before every governed side effect.
 * Heartbeat failure cancels the in-flight model/tool signal and fails closed. */
export async function withGoalExecutionLease(ledger, goalId, parentSignal, work) {
    if (parentSignal.aborted)
        throw new Error('goal execution cancelled before lease acquisition');
    const identity = `cli-${process.pid}-${randomUUID().slice(0, 8)}`;
    const lease = ledger.acquireGoalLease(goalId, identity, hostname(), process.pid, GOAL_LEASE_TTL_MS);
    const controller = new AbortController();
    const signal = AbortSignal.any([parentSignal, controller.signal]);
    let lost = null;
    const assertOwned = () => {
        if (lost)
            throw lost;
        ledger.assertGoalLease(goalId, lease.token);
        // A user cancellation is not lease loss: allow the supervisor to record its
        // ordinary paused checkpoint. A lease error still fails closed above.
    };
    const timer = setInterval(() => {
        if (lost || signal.aborted)
            return;
        try {
            ledger.renewGoalLease(goalId, lease.token, GOAL_LEASE_TTL_MS);
        }
        catch (error) {
            lost = error instanceof Error ? error : new Error(String(error));
            controller.abort(lost);
        }
    }, GOAL_LEASE_HEARTBEAT_MS);
    timer.unref();
    try {
        const result = await work({ signal, assertOwned });
        if (lost)
            throw lost;
        if (!parentSignal.aborted)
            assertOwned();
        return result;
    }
    finally {
        clearInterval(timer);
        // A late owner cannot erase a replacement lease.
        ledger.releaseGoalLease(goalId, lease.token);
    }
}
