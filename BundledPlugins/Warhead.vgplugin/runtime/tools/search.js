/** Search runs outside the CLI thread so regexes and filesystem traversal can
 * be interrupted even when the underlying operation cannot cooperate. */
import { Worker } from 'node:worker_threads';
export async function runSearch(name, args, ctx) {
    if (ctx.signal.aborted)
        return { ok: false, output: '', error: 'Search cancelled' };
    const limits = ctx.config().limits;
    const timeoutMs = limits?.perToolTimeoutMs?.[name] ?? limits?.toolTimeoutMs ?? 30_000;
    const url = new URL('./search-worker' + (import.meta.url.endsWith('.ts') ? '.ts' : '.js'), import.meta.url);
    return new Promise(resolve => {
        const worker = new Worker(url, {
            workerData: { name, args, cwd: ctx.cwd },
            // Workers execute a file; parent-only flags such as --input-type and
            // embedded runtime flags must not be reapplied to their entrypoint.
            execArgv: [],
            resourceLimits: { maxOldGenerationSizeMb: 96, maxYoungGenerationSizeMb: 16, stackSizeMb: 4 },
        });
        let settled = false;
        const finish = (outcome) => {
            if (settled)
                return;
            settled = true;
            clearTimeout(timer);
            ctx.signal.removeEventListener('abort', cancel);
            // Report completion only after the worker has stopped consuming resources.
            void worker.terminate().then(() => resolve(outcome), () => resolve(outcome));
        };
        const cancel = () => finish({ ok: false, output: '', error: 'Search cancelled' });
        const timer = setTimeout(() => finish({ ok: false, output: '', error: `Search timed out after ${timeoutMs}ms; narrow the path or simplify the pattern` }), timeoutMs);
        worker.once('message', finish);
        worker.once('error', error => finish({ ok: false, output: '', error: `Search failed: ${error.message}` }));
        worker.once('exit', code => finish({ ok: false, output: '', error: `Search worker exited (${code})` }));
        ctx.signal.addEventListener('abort', cancel, { once: true });
        if (ctx.signal.aborted)
            cancel();
    });
}
