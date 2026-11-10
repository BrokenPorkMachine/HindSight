/**
 * Transactional goal ledger.
 *
 * SQLite via `node:sqlite` (built in; no native module to compile). One ledger
 * per workspace so `status`/`resume` can discover goals without a global index.
 *
 * The ledger, not the conversation, is the authoritative record. Every mutation
 * that matters — plan insertion, task transition, tool intent, evidence,
 * checkpoint — is a transaction, so a process kill leaves either the previous
 * state or the next one, never a half-applied step.
 */
import { DatabaseSync } from 'node:sqlite';
import { createHash, randomUUID } from 'node:crypto';
import { chmodSync, mkdirSync, realpathSync } from 'node:fs';
import { homedir, hostname } from 'node:os';
import { dirname, join } from 'node:path';
import { DEFAULT_GOAL_BUDGETS } from './types.js';
const SCHEMA_VERSION = 4;
const MIGRATIONS = [
    (db) => db.exec(`
  CREATE TABLE goals (
    id TEXT PRIMARY KEY,
    workspace TEXT NOT NULL,
    objective TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    branch TEXT,
    commit_sha TEXT,
    budgets TEXT NOT NULL,
    cycles INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    blocker TEXT,
    completion_evidence_id TEXT
  );
  CREATE INDEX goals_workspace ON goals(workspace, updated_at DESC);

  CREATE TABLE acceptance (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    description TEXT NOT NULL,
    kind TEXT NOT NULL,
    command TEXT,
    expected_exit INTEGER NOT NULL DEFAULT 0,
    timeout_ms INTEGER NOT NULL DEFAULT 120000,
    status TEXT NOT NULL DEFAULT 'pending',
    evidence_id TEXT
  );
  CREATE INDEX acceptance_goal ON acceptance(goal_id, position);

  CREATE TABLE tasks (
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    position INTEGER NOT NULL,
    milestone TEXT,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    policy TEXT NOT NULL,
    verifier TEXT NOT NULL,
    owner TEXT NOT NULL DEFAULT 'lead',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL,
    fingerprint TEXT,
    next_action TEXT,
    strategy_index INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    verification TEXT NOT NULL DEFAULT 'pending',
    evidence_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    PRIMARY KEY (goal_id, id)
  );
  CREATE INDEX tasks_goal ON tasks(goal_id, position);

  CREATE TABLE dependencies (
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL,
    depends_on TEXT NOT NULL,
    PRIMARY KEY (goal_id, task_id, depends_on),
    FOREIGN KEY (goal_id, task_id) REFERENCES tasks(goal_id, id) ON DELETE CASCADE,
    FOREIGN KEY (goal_id, depends_on) REFERENCES tasks(goal_id, id) ON DELETE CASCADE
  );

  CREATE TABLE attempts (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL,
    cycle INTEGER NOT NULL,
    strategy TEXT NOT NULL,
    fingerprint TEXT,
    status TEXT NOT NULL,
    stop_reason TEXT,
    summary TEXT NOT NULL DEFAULT '',
    diff_fingerprint TEXT,
    progressed INTEGER NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    FOREIGN KEY (goal_id, task_id) REFERENCES tasks(goal_id, id) ON DELETE CASCADE
  );
  CREATE INDEX attempts_task ON attempts(task_id, started_at);

  CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    task_id TEXT,
    attempt_id TEXT,
    at TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL
  );
  CREATE INDEX events_goal ON events(goal_id, id);

  CREATE TABLE tool_intents (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    call_id TEXT NOT NULL,
    name TEXT NOT NULL,
    args TEXT NOT NULL,
    path TEXT,
    before_hash TEXT,
    status TEXT NOT NULL,
    ok INTEGER,
    output TEXT NOT NULL DEFAULT '',
    error TEXT,
    started_at TEXT NOT NULL,
    ended_at TEXT
  );
  CREATE INDEX intents_attempt ON tool_intents(attempt_id, started_at);
  CREATE INDEX intents_goal ON tool_intents(goal_id, started_at);

  CREATE TABLE evidence (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    task_id TEXT,
    attempt_id TEXT,
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    detail TEXT NOT NULL,
    exit_code INTEGER,
    log_path TEXT,
    created_at TEXT NOT NULL
  );
  CREATE INDEX evidence_goal ON evidence(goal_id, created_at);
  CREATE INDEX evidence_task ON evidence(task_id, created_at);

  CREATE TABLE checkpoints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    cycle INTEGER NOT NULL,
    at TEXT NOT NULL,
    phase TEXT NOT NULL,
    task_id TEXT,
    attempt_id TEXT,
    note TEXT NOT NULL DEFAULT ''
  );
  CREATE INDEX checkpoints_goal ON checkpoints(goal_id, id DESC);

  CREATE TABLE task_files (
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL,
    path TEXT NOT NULL,
    before_hash TEXT,
    after_hash TEXT,
    changed INTEGER NOT NULL DEFAULT 0,
    checked_at TEXT NOT NULL,
    PRIMARY KEY (goal_id, task_id, path),
    FOREIGN KEY (goal_id, task_id) REFERENCES tasks(goal_id, id) ON DELETE CASCADE
  );

  CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
  );
  `),
    (db) => db.exec(`
  CREATE TABLE goal_leases (
    goal_id TEXT PRIMARY KEY REFERENCES goals(id) ON DELETE CASCADE,
    token TEXT NOT NULL,
    holder TEXT NOT NULL,
    holder_host TEXT NOT NULL,
    holder_pid INTEGER NOT NULL,
    acquired_at_ms INTEGER NOT NULL,
    renewed_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL
  );
  `),
    (db) => {
        // These fields were introduced at schema v3.  Keep the v1 table definition
        // historical: otherwise a fresh ledger creates them here and immediately
        // attempts to add them again in this migration. Some early ledger snapshots
        // already contain a subset despite their recorded schema version, so inspect
        // the table rather than treating SQLite's duplicate-column error as success.
        const existing = new Set(db.prepare('PRAGMA table_info(evidence)').all().map(column => column.name));
        for (const [column, sql] of [
            ['artifact_sha256', 'ALTER TABLE evidence ADD COLUMN artifact_sha256 TEXT;'],
            ['workspace_revision', 'ALTER TABLE evidence ADD COLUMN workspace_revision TEXT;'],
            ['provenance', "ALTER TABLE evidence ADD COLUMN provenance TEXT NOT NULL DEFAULT '{}';"],
        ]) {
            if (existing.has(column))
                continue;
            db.exec(sql);
        }
        db.exec(`
  CREATE TABLE IF NOT EXISTS workspace_claims (
    workspace_key TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    token TEXT NOT NULL,
    acquired_at_ms INTEGER NOT NULL
  );
  CREATE UNIQUE INDEX IF NOT EXISTS workspace_claims_goal ON workspace_claims(goal_id);
  CREATE TABLE IF NOT EXISTS task_assignments (
    goal_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    assignee TEXT NOT NULL,
    assigned_by TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    scope TEXT NOT NULL,
    state TEXT NOT NULL,
    PRIMARY KEY (goal_id, task_id),
    FOREIGN KEY (goal_id, task_id) REFERENCES tasks(goal_id, id) ON DELETE CASCADE
  );
  `);
    },
    (db) => db.exec(`
  CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    goal_id TEXT,
    sha256 TEXT NOT NULL UNIQUE,
    byte_size INTEGER NOT NULL,
    media_type TEXT,
    source_uri TEXT,
    acquisition_tool TEXT,
    acquired_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS artifacts_goal ON artifacts(goal_id);
  CREATE INDEX IF NOT EXISTS artifacts_sha ON artifacts(sha256);

  CREATE TABLE IF NOT EXISTS claims (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    task_id TEXT,
    statement TEXT NOT NULL,
    status TEXT NOT NULL,
    supporting_evidence_ids TEXT NOT NULL DEFAULT '[]',
    refuting_evidence_ids TEXT NOT NULL DEFAULT '[]',
    scope_artifact_hashes TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS claims_goal ON claims(goal_id);
  CREATE INDEX IF NOT EXISTS claims_task ON claims(task_id);

  CREATE TABLE IF NOT EXISTS claim_relations (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    claim_id TEXT NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
    related_evidence_id TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    note TEXT,
    created_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS claim_rel_goal ON claim_relations(goal_id);
  CREATE INDEX IF NOT EXISTS claim_rel_claim ON claim_relations(claim_id);

  CREATE TABLE IF NOT EXISTS verification_runs (
    id TEXT PRIMARY KEY,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    task_id TEXT,
    criterion_id TEXT,
    status TEXT NOT NULL,
    verifier_name TEXT NOT NULL,
    verifier_version TEXT NOT NULL,
    input_hashes TEXT NOT NULL DEFAULT '[]',
    environment_fingerprint TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NOT NULL,
    exit_code INTEGER,
    output_artifact_ids TEXT NOT NULL DEFAULT '[]',
    detail TEXT NOT NULL DEFAULT ''
  );
  CREATE INDEX IF NOT EXISTS verif_goal ON verification_runs(goal_id);
  CREATE INDEX IF NOT EXISTS verif_task ON verification_runs(task_id);

  CREATE TABLE IF NOT EXISTS dream_scopes (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    allowed_rooms TEXT NOT NULL DEFAULT '[]',
    allowed_source_ids TEXT NOT NULL DEFAULT '[]',
    allowed_path_globs TEXT NOT NULL DEFAULT '[]',
    tier_ceilings TEXT NOT NULL DEFAULT '{}',
    provider_policy TEXT NOT NULL DEFAULT '{}',
    schedule TEXT,
    enabled INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'disabled',
    token_limit INTEGER,
    time_limit_ms INTEGER,
    tool_limit INTEGER,
    cost_limit_usd REAL,
    expires_at TEXT,
    revoked_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS dream_findings (
    id TEXT PRIMARY KEY,
    scope_id TEXT NOT NULL REFERENCES dream_scopes(id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL,
    subject TEXT NOT NULL,
    severity TEXT NOT NULL,
    source_event_ids TEXT NOT NULL DEFAULT '[]',
    source_hashes TEXT NOT NULL DEFAULT '[]',
    affected_projects TEXT NOT NULL DEFAULT '[]',
    inferred_claim INTEGER NOT NULL DEFAULT 0,
    verified_evidence INTEGER NOT NULL DEFAULT 0,
    contradictions TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'proposed',
    deduped_from TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS dream_findings_scope ON dream_findings(scope_id);
  CREATE INDEX IF NOT EXISTS dream_findings_fp ON dream_findings(fingerprint);

  CREATE TABLE IF NOT EXISTS dream_tasks (
    id TEXT PRIMARY KEY,
    scope_id TEXT NOT NULL REFERENCES dream_scopes(id) ON DELETE CASCADE,
    goal_id TEXT,
    finding_ids TEXT NOT NULL DEFAULT '[]',
    base_commit_sha TEXT NOT NULL,
    target_apps TEXT NOT NULL DEFAULT '[]',
    authorized_operations TEXT NOT NULL DEFAULT '[]',
    owner TEXT,
    lease_token TEXT,
    bounded_plan TEXT NOT NULL DEFAULT '{}',
    acceptance_criteria TEXT NOT NULL DEFAULT '[]',
    cost_tokens INTEGER NOT NULL DEFAULT 0,
    cost_time_ms INTEGER NOT NULL DEFAULT 0,
    cost_tool_calls INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'proposed',
    cancellation_reason TEXT,
    recovery_state TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS dream_tasks_scope ON dream_tasks(scope_id);
  CREATE INDEX IF NOT EXISTS dream_tasks_goal ON dream_tasks(goal_id);

  CREATE TABLE IF NOT EXISTS dream_proposals (
    id TEXT PRIMARY KEY,
    dream_task_id TEXT NOT NULL REFERENCES dream_tasks(id) ON DELETE CASCADE,
    branch_name TEXT NOT NULL,
    worktree_path TEXT,
    commits TEXT NOT NULL DEFAULT '[]',
    pr_number INTEGER,
    pr_url TEXT,
    head_sha TEXT,
    base_sha TEXT NOT NULL,
    file_manifest TEXT NOT NULL DEFAULT '[]',
    test_evidence_ids TEXT NOT NULL DEFAULT '[]',
    review_evidence_ids TEXT NOT NULL DEFAULT '[]',
    risk_labels TEXT NOT NULL DEFAULT '[]',
    rollback_instructions TEXT,
    provenance_manifest_hash TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS dream_promotions (
    id TEXT PRIMARY KEY,
    proposal_artifact_id TEXT NOT NULL REFERENCES dream_proposals(id) ON DELETE CASCADE,
    reviewer_id TEXT NOT NULL,
    authorized_operation TEXT NOT NULL,
    pr_number INTEGER,
    head_sha TEXT NOT NULL,
    base_sha TEXT NOT NULL,
    target_branch TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    decision TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    code_owner_confirmation INTEGER NOT NULL DEFAULT 0,
    audit_id TEXT NOT NULL,
    created_at TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS dream_feed (
    id TEXT PRIMARY KEY,
    scope_id TEXT NOT NULL REFERENCES dream_scopes(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    actor_class TEXT NOT NULL,
    audience_scope TEXT NOT NULL DEFAULT '[]',
    finding_id TEXT,
    dream_task_id TEXT,
    proposal_artifact_id TEXT,
    evidence_ids TEXT NOT NULL DEFAULT '[]',
    verification_status TEXT NOT NULL DEFAULT 'unverified',
    delivery_cursor INTEGER NOT NULL DEFAULT 0,
    payload TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS dream_feed_scope ON dream_feed(scope_id);
  CREATE INDEX IF NOT EXISTS dream_feed_cursor ON dream_feed(scope_id, delivery_cursor);
  `),
];
function now() {
    return new Date().toISOString();
}
function id(prefix) {
    return `${prefix}-${Date.now().toString(36)}-${randomUUID().slice(0, 8)}`;
}
function bool(value) {
    return value === 1 || value === true;
}
function safeJson(value, fallback) {
    if (typeof value !== 'string' || value === '')
        return fallback;
    try {
        return JSON.parse(value);
    }
    catch {
        return fallback;
    }
}
function workspaceKey(workspace) {
    return createHash('sha256').update(workspace).digest('hex').slice(0, 16);
}
export function goalStateRoot() {
    const base = process.env['XDG_STATE_HOME'] ?? join(homedir(), '.local', 'state');
    return join(base, 'warhead', 'goals');
}
/** Default ledger for a workspace. `WARHEAD_GOAL_DB` exists for tests and
 *  operators who keep state on a different volume. */
export function ledgerPathFor(workspace) {
    const override = process.env['WARHEAD_GOAL_DB'];
    if (override)
        return override;
    const resolved = (() => { try {
        return realpathSync(workspace);
    }
    catch {
        return workspace;
    } })();
    return join(goalStateRoot(), workspaceKey(resolved), 'ledger.db');
}
function leaseFromRow(row) {
    return {
        goalId: String(row['goal_id']), token: String(row['token']),
        holder: String(row['holder']), holderHost: String(row['holder_host']),
        holderPid: Number(row['holder_pid']), acquiredAtMs: Number(row['acquired_at_ms']),
        renewedAtMs: Number(row['renewed_at_ms']), expiresAtMs: Number(row['expires_at_ms']),
    };
}
export class GoalLedger {
    path;
    db;
    closed = false;
    constructor(path) {
        this.path = path;
        mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
        this.db = new DatabaseSync(path);
        try {
            chmodSync(path, 0o600);
        }
        catch { /* best effort on exotic filesystems */ }
        this.db.exec('PRAGMA journal_mode = WAL');
        this.db.exec('PRAGMA foreign_keys = ON');
        this.db.exec('PRAGMA busy_timeout = 5000');
        this.migrate();
    }
    static open(workspace, path = ledgerPathFor(workspace)) {
        return new GoalLedger(path);
    }
    close() {
        if (this.closed)
            return;
        this.closed = true;
        try {
            this.db.close();
        }
        catch { /* already closed */ }
    }
    migrate() {
        const current = Number(this.db.prepare('PRAGMA user_version').get()?.user_version ?? 0);
        if (current > SCHEMA_VERSION) {
            throw new Error(`goal ledger schema ${current} is newer than this Warhead build (${SCHEMA_VERSION})`);
        }
        for (let version = current; version < SCHEMA_VERSION; version++) {
            const migration = MIGRATIONS[version];
            if (!migration)
                throw new Error(`missing goal ledger migration ${version + 1}`);
            this.transaction(() => {
                migration(this.db);
                this.db.exec(`PRAGMA user_version = ${version + 1}`);
            });
        }
        this.db.prepare('INSERT OR IGNORE INTO meta(key, value) VALUES (?, ?)').run('schema_version', String(SCHEMA_VERSION));
    }
    transaction(work) {
        if (this.closed)
            throw new Error('goal ledger is closed');
        this.db.exec('BEGIN IMMEDIATE');
        try {
            const result = work();
            this.db.exec('COMMIT');
            return result;
        }
        catch (error) {
            try {
                this.db.exec('ROLLBACK');
            }
            catch { /* transaction already gone */ }
            throw error;
        }
    }
    workspaceClaimKey(workspace) {
        // Canonicalize every workspace before claiming, so path aliases cannot
        // bypass the shared SQLite workspace fence.
        const canonical = realpathSync(workspace);
        return createHash('sha256').update(canonical).digest('hex');
    }
    getWorkspaceClaim(workspace) {
        const row = this.db.prepare('SELECT goal_id, token FROM workspace_claims WHERE workspace_key = ?')
            .get(this.workspaceClaimKey(workspace));
        return row ? { goalId: row.goal_id, token: row.token } : null;
    }
    // --- per-goal execution ownership ----------------------------------------
    // Leases are advisory for ordinary SQL readers; only supervised execution
    // paths acquire them. Never silently steal an expired lease.
    getGoalLease(goalId) {
        const row = this.db.prepare('SELECT * FROM goal_leases WHERE goal_id = ?').get(goalId);
        return row ? leaseFromRow(row) : null;
    }
    acquireGoalLease(goalId, holder, host, pid, ttlMs = 30_000, at = Date.now()) {
        if (!holder.trim() || !host.trim() || !Number.isSafeInteger(pid) || pid < 1 ||
            !Number.isSafeInteger(ttlMs) || ttlMs < 10_000 || ttlMs > 300_000 ||
            !Number.isSafeInteger(at) || at < 0)
            throw new Error('invalid goal lease arguments');
        return this.transaction(() => {
            if (!this.getGoal(goalId))
                throw new Error('goal not found');
            const prior = this.getGoalLease(goalId);
            if (prior)
                throw new Error(`goal already leased by ${prior.holder} (${at >= prior.expiresAtMs ? 'expired; manual recovery required' : 'active'})`);
            const workspace = this.getGoal(goalId).workspace;
            const key = this.workspaceClaimKey(workspace);
            const conflict = this.db.prepare('SELECT goal_id FROM workspace_claims WHERE workspace_key = ?').get(key);
            if (conflict)
                throw new Error(`workspace already owned by goal ${conflict.goal_id}; manual recovery may be required`);
            const token = randomUUID();
            this.db.prepare('INSERT INTO workspace_claims(workspace_key,goal_id,token,acquired_at_ms) VALUES (?,?,?,?)')
                .run(key, goalId, token, at);
            this.db.prepare(`INSERT INTO goal_leases
        (goal_id, token, holder, holder_host, holder_pid, acquired_at_ms, renewed_at_ms, expires_at_ms)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)`)
                .run(goalId, token, holder, host, pid, at, at, at + ttlMs);
            this.appendEvent(goalId, null, null, 'goal.lease_acquired', { holder, host, pid });
            return this.getGoalLease(goalId);
        });
    }
    renewGoalLease(goalId, token, ttlMs = 30_000, at = Date.now()) {
        if (!Number.isSafeInteger(ttlMs) || ttlMs < 10_000 || ttlMs > 300_000)
            throw new Error('invalid lease duration');
        return this.transaction(() => {
            const changed = this.db.prepare(`UPDATE goal_leases SET renewed_at_ms = ?, expires_at_ms = ?
        WHERE goal_id = ? AND token = ? AND expires_at_ms > ?`)
                .run(at, at + ttlMs, goalId, token, at);
            if (Number(changed.changes) !== 1)
                throw new Error('goal execution lease lost or expired');
            const goal = this.getGoal(goalId);
            if (this.getWorkspaceClaim(goal.workspace)?.token !== token)
                throw new Error('workspace ownership lost');
            return this.getGoalLease(goalId);
        });
    }
    assertGoalLease(goalId, token, at = Date.now()) {
        const lease = this.getGoalLease(goalId);
        if (!lease || lease.token !== token || lease.expiresAtMs <= at)
            throw new Error('goal execution lease lost or expired');
        const goal = this.getGoal(goalId);
        if (!goal || this.getWorkspaceClaim(goal.workspace)?.token !== token)
            throw new Error('workspace ownership lost');
    }
    releaseGoalLease(goalId, token) {
        return this.transaction(() => {
            const row = this.db.prepare('DELETE FROM goal_leases WHERE goal_id = ? AND token = ?').run(goalId, token);
            if (Number(row.changes) > 0) {
                this.db.prepare('DELETE FROM workspace_claims WHERE goal_id = ? AND token = ?').run(goalId, token);
                this.appendEvent(goalId, null, null, 'goal.lease_released', {});
                return true;
            }
            return false;
        });
    }
    /** Operator-only: expired lease recovery is not an automatic takeover.
     * A late process can still have external work in flight. Inspect the host
     * and mutation journal, then explicitly acknowledge this uncertainty. */
    recoverExpiredGoalLease(goalId, acknowledge, at = Date.now()) {
        if (acknowledge !== true)
            throw new Error('operator acknowledgement required');
        return this.transaction(() => {
            const lease = this.getGoalLease(goalId);
            if (!lease)
                return false;
            if (lease.expiresAtMs > at)
                throw new Error('lease still active');
            // A same-host process can be checked for liveness; a remote host cannot.
            if (lease.holderHost === hostname() && lease.holderPid > 0) {
                try {
                    process.kill(lease.holderPid, 0);
                    throw new Error('lease holder process still alive; refuse recovery');
                }
                catch (error) {
                    if (error.code !== 'ESRCH')
                        throw error;
                }
            }
            const removed = this.db.prepare('DELETE FROM goal_leases WHERE goal_id = ? AND token = ?').run(goalId, lease.token);
            if (Number(removed.changes) !== 1)
                throw new Error('lease changed during recovery');
            this.db.prepare('DELETE FROM workspace_claims WHERE goal_id = ? AND token = ?').run(goalId, lease.token);
            this.appendEvent(goalId, null, null, 'goal.lease_recovered', {
                previousHolder: lease.holder, previousHost: lease.holderHost,
                previousPid: lease.holderPid, expiredAtMs: lease.expiresAtMs,
                warning: 'Operator acknowledged that side effects may still be in flight',
            });
            return true;
        });
    }
    // --- goals ---------------------------------------------------------------
    createGoal(input) {
        const stamp = now();
        const idValue = id('goal');
        const budgets = { ...DEFAULT_GOAL_BUDGETS, ...input.budgets };
        this.transaction(() => {
            this.db.prepare(`INSERT INTO goals
        (id, workspace, objective, status, created_at, updated_at, branch, commit_sha, budgets)
        VALUES (?, ?, ?, 'planning', ?, ?, ?, ?, ?)`)
                .run(idValue, input.workspace, input.objective, stamp, stamp, input.branch, input.commit, JSON.stringify(budgets));
            let position = 0;
            for (const criterion of input.plan.acceptance) {
                this.db.prepare(`INSERT INTO acceptance
          (id, goal_id, position, description, kind, command, expected_exit, timeout_ms, status)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending')`)
                    .run(id('acc'), idValue, position++, criterion.description, criterion.command ? 'command' : 'tasks', criterion.command ?? null, criterion.expectedExit ?? 0, criterion.timeoutMs ?? 120_000);
            }
            for (const [index, task] of input.plan.tasks.entries()) {
                const taskId = task.id ?? `T${String(index + 1).padStart(3, '0')}`;
                const policy = task.verification?.policy ?? 'evidence';
                const verifier = {
                    commands: task.verification?.commands ?? [],
                    files: task.verification?.files ?? [],
                    checks: task.verification?.checks ?? [],
                };
                this.db.prepare(`INSERT INTO tasks
          (goal_id, id, position, milestone, title, description, status, policy, verifier, owner,
           attempts, max_attempts, fingerprint, next_action, strategy_index, last_error, verification, evidence_id, created_at, updated_at, completed_at)
          VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, 'lead', 0, ?, NULL, NULL, 0, NULL, 'pending', NULL, ?, ?, NULL)`)
                    .run(idValue, taskId, index, task.milestone ?? null, task.title, task.description ?? '', policy, JSON.stringify(verifier), budgets.maxTaskAttempts, stamp, stamp);
                for (const dependency of task.dependsOn ?? []) {
                    this.db.prepare('INSERT OR IGNORE INTO dependencies(goal_id, task_id, depends_on) VALUES (?, ?, ?)')
                        .run(idValue, taskId, dependency);
                }
            }
            this.appendEvent(idValue, null, null, 'goal.created', { objective: input.objective, tasks: input.plan.tasks.length, fallback: input.plan.fallbackReason ?? null });
        });
        const goal = this.getGoal(idValue);
        if (!goal)
            throw new Error('goal vanished immediately after creation');
        return goal;
    }
    getGoal(goalId) {
        const row = this.db.prepare('SELECT * FROM goals WHERE id = ?').get(goalId);
        return row ? rowToGoal(row) : null;
    }
    listGoals(workspace, limit = 50) {
        const rows = workspace
            ? this.db.prepare('SELECT * FROM goals WHERE workspace = ? ORDER BY updated_at DESC LIMIT ?').all(workspace, limit)
            : this.db.prepare('SELECT * FROM goals ORDER BY updated_at DESC LIMIT ?').all(limit);
        return rows.map(rowToGoal);
    }
    latestGoal(workspace, statuses) {
        const goals = this.listGoals(workspace);
        return goals.find(goal => !statuses || statuses.includes(goal.status)) ?? null;
    }
    setGoalStatus(goalId, status, blocker = null) {
        this.db.prepare('UPDATE goals SET status = ?, blocker = ?, updated_at = ? WHERE id = ?')
            .run(status, blocker, now(), goalId);
        this.appendEvent(goalId, null, null, 'goal.status', { status, blocker });
    }
    bumpGoal(goalId, delta) {
        this.db.prepare(`UPDATE goals SET
      cycles = cycles + ?, attempts = attempts + ?, output_tokens = output_tokens + ?,
      tool_calls = tool_calls + ?, updated_at = ? WHERE id = ?`)
            .run(delta.cycles ?? 0, delta.attempts ?? 0, delta.outputTokens ?? 0, delta.toolCalls ?? 0, now(), goalId);
    }
    completeGoal(goalId, evidenceId) {
        this.db.prepare('UPDATE goals SET status = ?, blocker = NULL, completion_evidence_id = ?, updated_at = ? WHERE id = ?')
            .run('completed', evidenceId, now(), goalId);
        this.appendEvent(goalId, null, null, 'goal.completed', { evidenceId });
    }
    // --- acceptance ----------------------------------------------------------
    listAcceptance(goalId) {
        return this.db.prepare('SELECT * FROM acceptance WHERE goal_id = ? ORDER BY position').all(goalId).map(rowToAcceptance);
    }
    setAcceptanceStatus(goalId, acceptanceId, status, evidenceId) {
        this.db.prepare('UPDATE acceptance SET status = ?, evidence_id = ? WHERE goal_id = ? AND id = ?')
            .run(status, evidenceId, goalId, acceptanceId);
    }
    // --- tasks ---------------------------------------------------------------
    listTasks(goalId) {
        return this.db.prepare('SELECT * FROM tasks WHERE goal_id = ? ORDER BY position').all(goalId).map(rowToTask);
    }
    getTask(taskId, goalId) {
        const row = goalId
            ? this.db.prepare('SELECT * FROM tasks WHERE goal_id = ? AND id = ?').get(goalId, taskId)
            : this.db.prepare('SELECT * FROM tasks WHERE id = ?').get(taskId);
        return row ? rowToTask(row) : null;
    }
    listDependencies(goalId) {
        return this.db.prepare(`SELECT d.task_id, d.depends_on FROM dependencies d
      WHERE d.goal_id = ?`).all(goalId)
            .map(row => ({ taskId: String(row['task_id']), dependsOn: String(row['depends_on']) }));
    }
    updateTask(goalId, taskId, patch) {
        const columns = [];
        const values = [];
        const map = {
            status: 'status', attempts: 'attempts', fingerprint: 'fingerprint', nextAction: 'next_action',
            strategyIndex: 'strategy_index', lastError: 'last_error', verification: 'verification',
            evidenceId: 'evidence_id', owner: 'owner', completedAt: 'completed_at',
        };
        for (const [key, column] of Object.entries(map)) {
            if (!(key in patch))
                continue;
            columns.push(`${column} = ?`);
            const value = patch[key];
            values.push(value === undefined ? null : value);
        }
        if (columns.length === 0)
            return;
        columns.push('updated_at = ?');
        values.push(now(), goalId, taskId);
        this.db.prepare(`UPDATE tasks SET ${columns.join(', ')} WHERE goal_id = ? AND id = ?`).run(...values);
    }
    addRemediationTask(goalId, task, dependsOn) {
        const stamp = now();
        this.transaction(() => {
            this.db.prepare(`INSERT INTO tasks
        (goal_id, id, position, milestone, title, description, status, policy, verifier, owner,
         attempts, max_attempts, fingerprint, next_action, strategy_index, last_error, verification, evidence_id, created_at, updated_at, completed_at)
        VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, 'lead', 0, ?, NULL, NULL, 0, NULL, 'pending', NULL, ?, ?, NULL)`)
                .run(goalId, task.id, task.position, task.milestone, task.title, task.description, task.policy, JSON.stringify(task.verifier), task.maxAttempts, stamp, stamp);
            for (const dependency of dependsOn) {
                this.db.prepare('INSERT OR IGNORE INTO dependencies(goal_id, task_id, depends_on) VALUES (?, ?, ?)').run(goalId, task.id, dependency);
            }
            this.appendEvent(goalId, task.id, null, 'task.remediation', { title: task.title, dependsOn });
        });
    }
    // --- explicit participant task assignment --------------------------------
    // Assignments are metadata, not remote execution capabilities. Only an
    // operator holding the current live execution lease can assign tasks.
    assignTask(goalId, taskId, assignee, assignedBy, scope, leaseToken) {
        if (!/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,79}$/.test(assignee) ||
            !/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,79}$/.test(assignedBy) ||
            !['review', 'propose'].includes(scope))
            throw Error('invalid task assignment');
        return this.transaction(() => {
            this.assertGoalLease(goalId, leaseToken);
            const task = this.getTask(taskId, goalId);
            if (!task || task.status === 'completed')
                throw Error('task unavailable for assignment');
            if (this.getTaskAssignment(goalId, taskId)?.state === 'assigned')
                throw Error('task already assigned');
            const stamp = now();
            this.db.prepare(`INSERT INTO task_assignments
       (goal_id,task_id,assignee,assigned_by,assigned_at,scope,state)
       VALUES(?,?,?,?,?,?,'assigned')
       ON CONFLICT(goal_id,task_id) DO UPDATE SET assignee=excluded.assignee,
         assigned_by=excluded.assigned_by,assigned_at=excluded.assigned_at,
         scope=excluded.scope,state='assigned'`)
                .run(goalId, taskId, assignee, assignedBy, stamp, scope);
            this.appendEvent(goalId, taskId, null, 'task.assigned', { assignee, assignedBy, scope });
            return this.getTaskAssignment(goalId, taskId);
        });
    }
    getTaskAssignment(goalId, taskId) {
        const row = this.db.prepare('SELECT * FROM task_assignments WHERE goal_id=? AND task_id=?')
            .get(goalId, taskId);
        return row ? {
            goalId: String(row['goal_id']), taskId: String(row['task_id']),
            assignee: String(row['assignee']), assignedBy: String(row['assigned_by']),
            assignedAt: String(row['assigned_at']),
            scope: row['scope'], state: row['state'],
        } : null;
    }
    listTaskAssignments(goalId) {
        return this.listTasks(goalId).map(t => this.getTaskAssignment(goalId, t.id))
            .filter((x) => x !== null);
    }
    releaseTaskAssignment(goalId, taskId, leaseToken) {
        return this.transaction(() => {
            this.assertGoalLease(goalId, leaseToken);
            const row = this.db.prepare(`UPDATE task_assignments SET state='released'
        WHERE goal_id=? AND task_id=? AND state='assigned'`).run(goalId, taskId);
            if (Number(row.changes))
                this.appendEvent(goalId, taskId, null, 'task.assignment_released', {});
            return Number(row.changes) === 1;
        });
    }
    // --- attempts ------------------------------------------------------------
    recordAttempt(input) {
        const attempt = {
            ...input,
            id: id('attempt'),
            fingerprint: input.fingerprint ?? null,
            status: input.status ?? 'running',
            stopReason: null,
            summary: input.summary ?? '',
            diffFingerprint: null,
            progressed: false,
            endedAt: null,
        };
        this.db.prepare(`INSERT INTO attempts
      (id, goal_id, task_id, cycle, strategy, fingerprint, status, summary, started_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`)
            .run(attempt.id, attempt.goalId, attempt.taskId, attempt.cycle, attempt.strategy, attempt.fingerprint, attempt.status, attempt.summary, attempt.startedAt);
        return attempt;
    }
    finishAttempt(attemptId, patch) {
        this.db.prepare(`UPDATE attempts SET status = ?, stop_reason = ?, summary = ?, fingerprint = ?,
      diff_fingerprint = ?, progressed = ?, ended_at = ? WHERE id = ?`)
            .run(patch.status, patch.stopReason, patch.summary, patch.fingerprint ?? null, patch.diffFingerprint ?? null, patch.progressed ? 1 : 0, now(), attemptId);
    }
    listAttempts(goalId, taskId) {
        return (taskId
            ? this.db.prepare('SELECT * FROM attempts WHERE goal_id = ? AND task_id = ? ORDER BY started_at').all(goalId, taskId)
            : this.db.prepare('SELECT * FROM attempts WHERE goal_id = ? ORDER BY started_at').all(goalId)).map(rowToAttempt);
    }
    // --- events / evidence ---------------------------------------------------
    appendEvent(goalId, taskId, attemptId, kind, payload) {
        this.db.prepare('INSERT INTO events(goal_id, task_id, attempt_id, at, kind, payload) VALUES (?, ?, ?, ?, ?, ?)')
            .run(goalId, taskId, attemptId, now(), kind, JSON.stringify(payload));
    }
    listEvents(goalId, limit = 200) {
        return this.db.prepare('SELECT * FROM events WHERE goal_id = ? ORDER BY id DESC LIMIT ?').all(goalId, limit)
            .map(row => ({
            id: Number(row['id']), goalId: String(row['goal_id']),
            taskId: row['task_id'] ?? null,
            attemptId: row['attempt_id'] ?? null,
            at: String(row['at']), kind: String(row['kind']),
            payload: safeJson(row['payload'], {}),
        }));
    }
    addEvidence(input) {
        const evidence = {
            ...input,
            artifactSha256: input.artifactSha256 ?? null,
            workspaceRevision: input.workspaceRevision ?? null,
            provenance: input.provenance ?? {},
            id: id('ev'), createdAt: input.createdAt ?? now(),
        };
        this.db.prepare(`INSERT INTO evidence
      (id, goal_id, task_id, attempt_id, kind, label, detail, exit_code, log_path,
       artifact_sha256, workspace_revision, provenance, created_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
            .run(evidence.id, evidence.goalId, evidence.taskId, evidence.attemptId, evidence.kind, evidence.label, evidence.detail, evidence.exitCode, evidence.logPath, evidence.artifactSha256, evidence.workspaceRevision, JSON.stringify(evidence.provenance), evidence.createdAt);
        this.appendEvent(evidence.goalId, evidence.taskId, evidence.attemptId, 'evidence.added', {
            evidenceId: evidence.id, kind: evidence.kind, label: evidence.label, exitCode: evidence.exitCode,
        });
        return evidence;
    }
    listEvidence(goalId, options = {}) {
        const limit = options.limit ?? 200;
        if (options.all) {
            return (options.taskId
                ? this.db.prepare('SELECT id, goal_id, task_id, attempt_id, kind, label, detail, exit_code, log_path, artifact_sha256, workspace_revision, provenance, created_at FROM evidence WHERE goal_id = ? AND task_id = ? ORDER BY created_at DESC').all(goalId, options.taskId)
                : this.db.prepare('SELECT id, goal_id, task_id, attempt_id, kind, label, detail, exit_code, log_path, artifact_sha256, workspace_revision, provenance, created_at FROM evidence WHERE goal_id = ? ORDER BY created_at DESC').all(goalId)).map(rowToEvidence);
        }
        return (options.taskId
            ? this.db.prepare('SELECT id, goal_id, task_id, attempt_id, kind, label, detail, exit_code, log_path, artifact_sha256, workspace_revision, provenance, created_at FROM evidence WHERE goal_id = ? AND task_id = ? ORDER BY created_at DESC LIMIT ?').all(goalId, options.taskId, limit)
            : this.db.prepare('SELECT id, goal_id, task_id, attempt_id, kind, label, detail, exit_code, log_path, artifact_sha256, workspace_revision, provenance, created_at FROM evidence WHERE goal_id = ? ORDER BY created_at DESC LIMIT ?').all(goalId, limit)).map(rowToEvidence);
    }
    countEvidence(goalId, kind) {
        const row = kind
            ? this.db.prepare('SELECT COUNT(*) AS n FROM evidence WHERE goal_id = ? AND kind = ?').get(goalId, kind)
            : this.db.prepare('SELECT COUNT(*) AS n FROM evidence WHERE goal_id = ?').get(goalId);
        return Number(row?.['n'] ?? 0);
    }
    addClaim(input) {
        const claim = {
            ...input,
            id: input.id ?? id('claim'),
            supportingEvidenceIds: input.supportingEvidenceIds ?? [],
            refutingEvidenceIds: input.refutingEvidenceIds ?? [],
            scopeArtifactHashes: input.scopeArtifactHashes ?? [],
            createdAt: input.createdAt ?? now(),
            updatedAt: input.updatedAt ?? now(),
        };
        this.db.prepare(`INSERT INTO claims
      (id, goal_id, task_id, statement, status, supporting_evidence_ids, refuting_evidence_ids, scope_artifact_hashes, created_at, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
            .run(claim.id, claim.goalId, claim.taskId ?? null, claim.statement, claim.status, JSON.stringify(claim.supportingEvidenceIds), JSON.stringify(claim.refutingEvidenceIds), JSON.stringify(claim.scopeArtifactHashes), claim.createdAt, claim.updatedAt);
        return claim;
    }
    listClaims(goalId, taskId) {
        return (taskId
            ? this.db.prepare('SELECT * FROM claims WHERE goal_id = ? AND task_id = ? ORDER BY created_at').all(goalId, taskId)
            : this.db.prepare('SELECT * FROM claims WHERE goal_id = ? ORDER BY created_at').all(goalId)).map(rowToClaim);
    }
    updateClaim(claimId, patch) {
        const sets = [];
        const values = [];
        if (patch.statement !== undefined) {
            sets.push('statement = ?');
            values.push(patch.statement);
        }
        if (patch.status !== undefined) {
            sets.push('status = ?');
            values.push(patch.status);
        }
        if (patch.supportingEvidenceIds !== undefined) {
            sets.push('supporting_evidence_ids = ?');
            values.push(JSON.stringify(patch.supportingEvidenceIds));
        }
        if (patch.refutingEvidenceIds !== undefined) {
            sets.push('refuting_evidence_ids = ?');
            values.push(JSON.stringify(patch.refutingEvidenceIds));
        }
        if (patch.scopeArtifactHashes !== undefined) {
            sets.push('scope_artifact_hashes = ?');
            values.push(JSON.stringify(patch.scopeArtifactHashes));
        }
        sets.push('updated_at = ?');
        values.push(now());
        values.push(claimId);
        if (sets.length === 0)
            return;
        this.db.prepare(`UPDATE claims SET ${sets.join(', ')} WHERE id = ?`).run(...values);
    }
    addVerificationRun(input) {
        const record = {
            ...input,
            id: input.id ?? id('vrec'),
            inputHashes: input.inputHashes ?? [],
            outputArtifactIds: input.outputArtifactIds ?? [],
        };
        this.db.prepare(`INSERT INTO verification_runs
      (id, goal_id, task_id, criterion_id, status, verifier_name, verifier_version, input_hashes, environment_fingerprint, started_at, ended_at, exit_code, output_artifact_ids, detail)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
            .run(record.id, record.goalId, record.taskId ?? null, record.criterionId ?? null, record.status, record.verifier.name, record.verifier.version, JSON.stringify(record.inputHashes), record.environmentFingerprint, record.startedAt, record.endedAt, record.exitCode, JSON.stringify(record.outputArtifactIds), record.detail);
        return record;
    }
    listVerificationRuns(goalId, taskId) {
        return (taskId
            ? this.db.prepare('SELECT * FROM verification_runs WHERE goal_id = ? AND task_id = ? ORDER BY started_at').all(goalId, taskId)
            : this.db.prepare('SELECT * FROM verification_runs WHERE goal_id = ? ORDER BY started_at').all(goalId)).map(rowToVerificationRun);
    }
    // --- tool intents --------------------------------------------------------
    recordIntent(input) {
        const intent = {
            ...input, id: id('intent'), status: 'started', ok: null, output: '', error: null,
            startedAt: now(), endedAt: null,
        };
        this.db.prepare(`INSERT INTO tool_intents
      (id, goal_id, task_id, attempt_id, call_id, name, args, path, before_hash, status, started_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'started', ?)`)
            .run(intent.id, intent.goalId, intent.taskId, intent.attemptId, intent.callId, intent.name, intent.args, intent.path, intent.beforeHash, intent.startedAt);
        return intent;
    }
    finishIntent(intentId, status, ok, output, error) {
        this.db.prepare('UPDATE tool_intents SET status = ?, ok = ?, output = ?, error = ?, ended_at = ? WHERE id = ?')
            .run(status, ok === null ? null : (ok ? 1 : 0), output.slice(0, 4000), error, now(), intentId);
    }
    listOpenIntents(goalId) {
        return this.db.prepare("SELECT * FROM tool_intents WHERE goal_id = ? AND status = 'started' ORDER BY started_at")
            .all(goalId).map(rowToIntent);
    }
    /** Replay fences are read without a recency limit: an old uncertain mutation
     * must not become executable merely because the goal accumulated events. */
    listReplayFencedIntents(goalId) {
        return this.db.prepare("SELECT * FROM tool_intents WHERE goal_id = ? AND status IN ('unknown', 'denied') ORDER BY started_at")
            .all(goalId).map(rowToIntent);
    }
    listIntents(goalId, options = {}) {
        const limit = options.limit ?? 100;
        if (options.attemptId) {
            return this.db.prepare('SELECT * FROM tool_intents WHERE goal_id = ? AND attempt_id = ? ORDER BY started_at DESC LIMIT ?')
                .all(goalId, options.attemptId, limit).map(rowToIntent);
        }
        if (options.taskId) {
            return this.db.prepare('SELECT * FROM tool_intents WHERE goal_id = ? AND task_id = ? ORDER BY started_at DESC LIMIT ?')
                .all(goalId, options.taskId, limit).map(rowToIntent);
        }
        return this.db.prepare('SELECT * FROM tool_intents WHERE goal_id = ? ORDER BY started_at DESC LIMIT ?')
            .all(goalId, limit).map(rowToIntent);
    }
    // --- checkpoints and files ----------------------------------------------
    saveCheckpoint(input) {
        this.db.prepare('INSERT INTO checkpoints(goal_id, cycle, at, phase, task_id, attempt_id, note) VALUES (?, ?, ?, ?, ?, ?, ?)')
            .run(input.goalId, input.cycle, now(), input.phase, input.taskId, input.attemptId, input.note);
    }
    latestCheckpoint(goalId) {
        const row = this.db.prepare('SELECT * FROM checkpoints WHERE goal_id = ? ORDER BY id DESC LIMIT 1').get(goalId);
        return row ? {
            id: Number(row['id']), goalId: String(row['goal_id']), cycle: Number(row['cycle']),
            at: String(row['at']), phase: String(row['phase']),
            taskId: row['task_id'] ?? null,
            attemptId: row['attempt_id'] ?? null,
            note: String(row['note']),
        } : null;
    }
    setTaskFile(goalId, taskId, file) {
        this.db.prepare(`INSERT INTO task_files(goal_id, task_id, path, before_hash, after_hash, changed, checked_at)
      VALUES (?, ?, ?, ?, ?, ?, ?)
      ON CONFLICT(goal_id, task_id, path) DO UPDATE SET
        before_hash = COALESCE(task_files.before_hash, excluded.before_hash),
        after_hash = excluded.after_hash,
        changed = excluded.changed,
        checked_at = excluded.checked_at`)
            .run(goalId, taskId, file.path, file.beforeHash, file.afterHash, file.changed ? 1 : 0, now());
    }
    listTaskFiles(goalId, taskId) {
        return this.db.prepare('SELECT * FROM task_files WHERE goal_id = ? AND task_id = ? ORDER BY path').all(goalId, taskId).map(row => ({
            taskId: String(row['task_id']), path: String(row['path']),
            beforeHash: row['before_hash'] ?? null,
            afterHash: row['after_hash'] ?? null,
            changed: bool(row['changed']), checkedAt: String(row['checked_at']),
        }));
    }
    meta(key) {
        const row = this.db.prepare('SELECT value FROM meta WHERE key = ?').get(key);
        return row ? String(row['value']) : null;
    }
    setMeta(key, value) {
        this.db.prepare('INSERT INTO meta(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value')
            .run(key, value);
    }
}
// --- row mapping -----------------------------------------------------------
function rowToGoal(row) {
    return {
        id: String(row['id']),
        workspace: String(row['workspace']),
        objective: String(row['objective']),
        status: String(row['status']),
        createdAt: String(row['created_at']),
        updatedAt: String(row['updated_at']),
        branch: row['branch'] ?? null,
        commit: row['commit_sha'] ?? null,
        budgets: { ...DEFAULT_GOAL_BUDGETS, ...safeJson(row['budgets'], {}) },
        cycles: Number(row['cycles']), attempts: Number(row['attempts']),
        outputTokens: Number(row['output_tokens']), toolCalls: Number(row['tool_calls']),
        blocker: row['blocker'] ?? null,
        completionEvidenceId: row['completion_evidence_id'] ?? null,
    };
}
function rowToAcceptance(row) {
    return {
        id: String(row['id']), goalId: String(row['goal_id']), position: Number(row['position']),
        description: String(row['description']), kind: String(row['kind']),
        command: row['command'] ?? null,
        expectedExit: Number(row['expected_exit']), timeoutMs: Number(row['timeout_ms']),
        status: String(row['status']),
        evidenceId: row['evidence_id'] ?? null,
    };
}
function rowToTask(row) {
    return {
        id: String(row['id']), goalId: String(row['goal_id']), position: Number(row['position']),
        milestone: row['milestone'] ?? null,
        title: String(row['title']), description: String(row['description']),
        status: String(row['status']),
        policy: String(row['policy']),
        verifier: safeJson(row['verifier'], { commands: [], files: [], checks: [] }),
        owner: String(row['owner']), attempts: Number(row['attempts']), maxAttempts: Number(row['max_attempts']),
        fingerprint: row['fingerprint'] ?? null,
        nextAction: row['next_action'] ?? null,
        strategyIndex: Number(row['strategy_index']),
        lastError: row['last_error'] ?? null,
        verification: String(row['verification']),
        evidenceId: row['evidence_id'] ?? null,
        createdAt: String(row['created_at']), updatedAt: String(row['updated_at']),
        completedAt: row['completed_at'] ?? null,
    };
}
function rowToAttempt(row) {
    return {
        id: String(row['id']), goalId: String(row['goal_id']), taskId: String(row['task_id']),
        cycle: Number(row['cycle']), strategy: String(row['strategy']),
        fingerprint: row['fingerprint'] ?? null,
        status: String(row['status']),
        stopReason: row['stop_reason'] ?? null,
        summary: String(row['summary']),
        diffFingerprint: row['diff_fingerprint'] ?? null,
        progressed: bool(row['progressed']),
        startedAt: String(row['started_at']), endedAt: row['ended_at'] ?? null,
    };
}
function rowToIntent(row) {
    return {
        id: String(row['id']), goalId: String(row['goal_id']), taskId: String(row['task_id']),
        attemptId: String(row['attempt_id']), callId: String(row['call_id']), name: String(row['name']),
        args: String(row['args']), path: row['path'] ?? null,
        beforeHash: row['before_hash'] ?? null,
        status: String(row['status']),
        ok: row['ok'] === null ? null : bool(row['ok']),
        output: String(row['output']), error: row['error'] ?? null,
        startedAt: String(row['started_at']), endedAt: row['ended_at'] ?? null,
    };
}
function rowToEvidence(row) {
    return {
        id: String(row['id']), goalId: String(row['goal_id']),
        taskId: row['task_id'] ?? null,
        attemptId: row['attempt_id'] ?? null,
        kind: String(row['kind']),
        label: String(row['label']), detail: String(row['detail']),
        exitCode: row['exit_code'] === null ? null : Number(row['exit_code']),
        logPath: row['log_path'] ?? null,
        artifactSha256: row['artifact_sha256'] ?? null,
        workspaceRevision: row['workspace_revision'] ?? null,
        provenance: safeJson(row['provenance'], {}),
        createdAt: String(row['created_at']),
    };
}
function rowToClaim(row) {
    return {
        id: String(row['id']),
        goalId: String(row['goal_id']),
        taskId: row['task_id'] ?? null,
        statement: String(row['statement']),
        status: String(row['status']),
        supportingEvidenceIds: safeJson(row['supporting_evidence_ids'], []),
        refutingEvidenceIds: safeJson(row['refuting_evidence_ids'], []),
        scopeArtifactHashes: safeJson(row['scope_artifact_hashes'], []),
        createdAt: String(row['created_at']),
        updatedAt: String(row['updated_at']),
    };
}
function rowToVerificationRun(row) {
    return {
        id: String(row['id']),
        goalId: String(row['goal_id']),
        taskId: row['task_id'] ?? null,
        criterionId: row['criterion_id'] ?? null,
        status: String(row['status']),
        verifier: { name: String(row['verifier_name']), version: String(row['verifier_version']) },
        inputHashes: safeJson(row['input_hashes'], []),
        environmentFingerprint: String(row['environment_fingerprint']),
        startedAt: String(row['started_at']),
        endedAt: String(row['ended_at']),
        exitCode: row['exit_code'] === null ? null : Number(row['exit_code']),
        outputArtifactIds: safeJson(row['output_artifact_ids'], []),
        detail: String(row['detail']),
    };
}
