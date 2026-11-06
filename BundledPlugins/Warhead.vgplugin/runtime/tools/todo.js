/**
 * todo — track multi-step work.
 *
 * The list is the agent's own scratch plan. It lives in ToolContext rather than
 * module scope so it resets with the session and stays inspectable by tests.
 * The tool is idempotent from the model's point of view: each call replaces the
 * list, which is what the model expects when it "updates the todo list".
 */
import { reqArray, optString, ArgError } from './args.js';
export function createTodoStore() {
    let items = [];
    let seq = 0;
    return {
        list: () => items.map(i => ({ ...i })),
        nextId: () => `t${++seq}`,
        write(next) {
            const previous = items;
            const built = next.map(n => {
                const existing = previous.find(p => p.content === n.content);
                return {
                    id: existing?.id ?? `t${++seq}`,
                    content: n.content,
                    status: n.status ?? 'pending',
                };
            });
            items = built;
            return items.map(i => ({ ...i }));
        },
    };
}
/** A read-only view of verified supervisor tasks for model-visible todo calls.
 * The model may propose progress, but only the goal ledger can certify it.
 * refresh() is called by the supervisor after reading persistent task state. */
export function createSupervisedTodoStore() {
    const underlying = createTodoStore();
    return {
        list: () => underlying.list(),
        nextId: () => underlying.nextId(),
        // Deliberately ignore model-written state. Returning the canonical list
        // makes the tool response reflect reality, not an unverified claim.
        write: () => underlying.list(),
        refresh: items => underlying.write(items),
    };
}
/**
 * Read-only projection of an authoritative, persistent goal.
 * An LLM may call the ordinary todo tool, but the supervised store reflects
 * only ledger state and never accepts completed status from model output.
 * A supervisor refreshes the projection after each ledger transaction.
 */
export function createGoalLedgerTodoStore(ledger, goalId) {
    const store = createSupervisedTodoStore();
    const refresh = () => {
        const goal = ledger.snapshot().goals.find(entry => entry.id === goalId);
        if (!goal)
            throw Error('goal not found');
        return store.refresh(goal.tasks.map(task => ({
            content: task.title,
            status: task.status === 'verified' ? 'completed'
                : task.status === 'in_progress' ? 'in_progress' : 'pending',
        })));
    };
    refresh();
    return { list: store.list, nextId: store.nextId, write: store.write, refresh };
}
const MARK = {
    pending: '[ ]',
    in_progress: '[~]',
    completed: '[x]',
};
export const todo = {
    mutates: false, // in-memory agent state, not the user's filesystem
    def: {
        name: 'todo',
        description: 'Create or update the task list for the current turn. Each call replaces the ' +
            'whole list, so send every task each time. Keep exactly one task in_progress ' +
            'while others are pending or completed. Mark a task completed as soon as it is ' +
            'done rather than batching at the end.',
        parameters: {
            type: 'object',
            properties: {
                todos: {
                    type: 'array',
                    description: 'The complete task list for this turn.',
                    items: {
                        type: 'object',
                        properties: {
                            content: { type: 'string', description: 'Task description' },
                            status: {
                                type: 'string',
                                enum: ['pending', 'in_progress', 'completed'],
                                description: 'Task state',
                            },
                        },
                        required: ['content'],
                    },
                },
            },
            required: ['todos'],
        },
    },
    async run(args, ctx) {
        let raw;
        try {
            raw = reqArray(args, 'todos');
        }
        catch (err) {
            return { ok: false, output: '', error: err instanceof ArgError ? err.message : String(err) };
        }
        const parsed = [];
        for (const entry of raw) {
            if (typeof entry === 'string') {
                parsed.push({ content: entry });
                continue;
            }
            if (typeof entry !== 'object' || entry === null)
                continue;
            const e = entry;
            const content = optString(e, 'content') ?? optString(e, 'task') ?? optString(e, 'description');
            if (!content)
                continue;
            const st = optString(e, 'status');
            const status = st === 'completed' || st === 'in_progress' ? st : st === 'pending' || st == null ? 'pending' : undefined;
            parsed.push(status ? { content, status } : { content });
        }
        if (parsed.length === 0) {
            return { ok: false, output: '', error: "parameter 'todos' contained no usable task entries" };
        }
        const stored = ctx.todos.write(parsed);
        return { ok: true, output: render(stored) };
    },
};
export function render(items) {
    if (items.length === 0)
        return 'Task list is empty.';
    const done = items.filter(i => i.status === 'completed').length;
    const lines = items.map(i => `${MARK[i.status]} ${i.content}`);
    lines.push(`(${done}/${items.length} complete)`);
    return lines.join('\n');
}
