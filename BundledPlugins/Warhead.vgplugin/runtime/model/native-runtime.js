/** Persistent, Warhead-owned macOS inference runtime. Communication uses a
 * private newline-delimited JSON stream over stdio; no local HTTP listener is
 * opened and no Ollama process is required. */
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { existsSync, statSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
export class NativeRuntime {
    child = null;
    nextId = 1;
    waiters = new Map();
    loadedModel = '';
    queue = Promise.resolve();
    resolveBinary;
    idleTimeoutMs;
    constructor(resolveBinary = ensureRuntimeBuilt, options = {}) {
        this.resolveBinary = resolveBinary;
        const configured = options.idleTimeoutMs ?? Number(process.env['WARHEAD_NATIVE_IDLE_TIMEOUT_MS']);
        this.idleTimeoutMs = Number.isSafeInteger(configured) && configured > 0 && configured <= 2_147_483_647 ? configured : 15 * 60 * 1000;
    }
    async *chat(model, messages, tools, signal, numPredict, style = {}) {
        const release = await this.acquire(signal);
        try {
            await this.load(model, signal);
            const body = {
                method: 'chat', model, messages: toWire(messages), tools: tools.map(tool => ({
                    type: 'function', function: { name: tool.name, description: tool.description, parameters: tool.parameters },
                })),
                numPredict: numPredict ?? 2048,
                // Ollama keeps the tag's real chat template in its manifest; the GGUF
                // often carries the base model's. Without this the prompt framing does
                // not match what the model was tuned for and tool calls degrade to prose.
                ...(style.template ? { template: style.template } : {}),
                ...(style.stops?.length ? { stops: style.stops } : {}),
            };
            for await (const event of this.request(body, signal)) {
                if (event.event === 'text' && typeof event['delta'] === 'string')
                    yield { type: 'text', delta: event['delta'] };
                else if (event.event === 'tool_call') {
                    const call = event['call'];
                    if (call && typeof call.name === 'string')
                        yield { type: 'tool_call', call };
                }
                else if (event.event === 'done') {
                    yield {
                        type: 'done',
                        evalCount: typeof event['evalCount'] === 'number' ? event['evalCount'] : 0,
                        promptEvalCount: typeof event['promptEvalCount'] === 'number' ? event['promptEvalCount'] : 0,
                    };
                }
                else if (event.event === 'error') {
                    throw new Error(typeof event['message'] === 'string' ? event['message'] : 'native runtime failed');
                }
            }
        }
        finally {
            release();
        }
    }
    async acquire(signal) {
        const previous = this.queue;
        let release;
        const gate = new Promise(resolve => { release = resolve; });
        this.queue = previous.then(() => gate);
        try {
            await abortable(previous, signal);
        }
        catch (error) {
            release();
            throw error;
        }
        return release;
    }
    async close() {
        const child = this.child;
        this.child = null;
        this.loadedModel = '';
        if (!child)
            return;
        if (child.exitCode !== null || child.signalCode !== null)
            return;
        // Cooperate with native Metal teardown. SIGTERM immediately after the
        // shutdown request previously pre-empted LlamaEngine.deinit.
        await new Promise(resolve => {
            const timer = setTimeout(() => child.kill('SIGTERM'), 3_000);
            const hard = setTimeout(() => child.kill('SIGKILL'), 5_000);
            child.once('exit', () => { clearTimeout(timer); clearTimeout(hard); resolve(); });
            child.once('error', () => { clearTimeout(timer); clearTimeout(hard); resolve(); });
            try {
                child.stdin.write('{"id":0,"method":"shutdown"}\n');
            }
            catch {
                child.kill('SIGTERM');
            }
        });
    }
    async load(model, signal) {
        if (this.loadedModel === model)
            return;
        for await (const event of this.request({ method: 'load', model }, signal)) {
            if (event.event === 'error')
                throw new Error(typeof event['message'] === 'string' ? event['message'] : 'model load failed');
        }
        this.loadedModel = model;
    }
    async *request(payload, signal) {
        if (signal?.aborted)
            throw new Error('native model request cancelled');
        const child = await this.ensureProcess(signal);
        if (signal?.aborted)
            throw new Error('native model request cancelled');
        const id = this.nextId++;
        const queue = [];
        let queuedBytes = 0;
        const MAX_QUEUED_BYTES = 8 * 1024 * 1024;
        let wake;
        let error;
        let finished = false;
        let timer;
        const resetDeadline = () => {
            clearTimeout(timer);
            timer = setTimeout(() => {
                error = new Error(`Native runtime timed out after ${this.idleTimeoutMs}ms without output`);
                finished = true;
                wake?.();
                wake = undefined;
                this.retire(child);
            }, this.idleTimeoutMs);
        };
        this.waiters.set(id, {
            child,
            push: event => {
                const bytes = Buffer.byteLength(JSON.stringify(event), 'utf8');
                if (queuedBytes + bytes > MAX_QUEUED_BYTES) {
                    error = new Error('Native inference output exceeded the 8 MiB pending-event limit');
                    finished = true;
                    wake?.();
                    wake = undefined;
                    this.retire(child);
                    return;
                }
                queuedBytes += bytes;
                resetDeadline();
                queue.push(event);
                if (event.event === 'done' || event.event === 'error')
                    finished = true;
                wake?.();
                wake = undefined;
            },
            fail: value => { error = value; finished = true; wake?.(); wake = undefined; },
        });
        const abort = () => {
            error = new Error('native model request cancelled');
            finished = true;
            wake?.();
            wake = undefined;
            // Generation runs inside the native process and cannot consume a second
            // request while busy. Retire it so cancellation stops compute and the next
            // turn starts a clean runtime.
            this.retire(child);
        };
        signal?.addEventListener('abort', abort, { once: true });
        try {
            resetDeadline();
            child.stdin.write(`${JSON.stringify({ ...payload, id })}\n`);
            while (!finished || queue.length > 0) {
                const next = queue.shift();
                if (next) {
                    queuedBytes -= Buffer.byteLength(JSON.stringify(next), 'utf8');
                    yield next;
                    continue;
                }
                if (error)
                    throw error;
                await new Promise(resolve => { wake = resolve; });
            }
            if (error)
                throw error;
        }
        finally {
            clearTimeout(timer);
            signal?.removeEventListener('abort', abort);
            this.waiters.delete(id);
            if (!finished)
                this.retire(child);
        }
    }
    retire(child) {
        // Detach immediately: the next turn must not reuse a dying runtime or its
        // loaded-model cache while waiting for the asynchronous exit event.
        if (this.child === child) {
            this.child = null;
            this.loadedModel = '';
        }
        if (child.exitCode !== null || child.signalCode !== null || child.killed)
            return;
        child.kill('SIGTERM');
        const timer = setTimeout(() => child.kill('SIGKILL'), 1500);
        timer.unref();
        child.once('exit', () => clearTimeout(timer));
    }
    async ensureProcess(signal) {
        if (this.child && this.child.exitCode === null && this.child.signalCode === null && !this.child.killed)
            return this.child;
        const binary = await abortable(this.resolveBinary(), signal);
        signal?.throwIfAborted();
        const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'pipe'] });
        this.child = child;
        // Bound individual protocol lines before readline can accumulate unbounded
        // data from a compromised or malfunctioning native subprocess.
        const MAX_PROTOCOL_LINE_BYTES = 2 * 1024 * 1024;
        let protocolBytes = 0;
        child.stdout.on('data', (chunk) => {
            const lastNewline = chunk.lastIndexOf(10);
            protocolBytes = lastNewline >= 0 ? chunk.length - lastNewline - 1 : protocolBytes + chunk.length;
            if (protocolBytes > MAX_PROTOCOL_LINE_BYTES) {
                const error = new Error('native runtime protocol line exceeded 2 MiB');
                for (const waiter of this.waiters.values())
                    if (waiter.child === child)
                        waiter.fail(error);
                this.retire(child);
            }
        });
        const lines = createInterface({ input: child.stdout });
        lines.on('line', line => {
            try {
                if (Buffer.byteLength(line, 'utf8') > MAX_PROTOCOL_LINE_BYTES)
                    throw new Error('oversized native runtime frame');
                const event = JSON.parse(line);
                this.waiters.get(event.id)?.push(event);
            }
            catch { /* ignore non-protocol output */ }
        });
        child.stderr.on('data', chunk => process.stderr.write(chunk));
        // A child that dies between spawn and the first write surfaces as EPIPE on
        // stdin; without a listener Node treats it as an unhandled 'error' and takes
        // the whole CLI down instead of reporting the failed request.
        child.stdin.on('error', error => {
            for (const waiter of this.waiters.values())
                if (waiter.child === child)
                    waiter.fail(error);
            this.retire(child);
        });
        const exited = () => {
            if (this.child === child) {
                this.child = null;
                this.loadedModel = '';
            }
            const error = new Error(`Warhead native runtime exited (${child.signalCode ?? child.exitCode ?? 'unknown'})`);
            for (const waiter of this.waiters.values())
                if (waiter.child === child)
                    waiter.fail(error);
        };
        child.on('error', exited);
        child.on('exit', exited);
        return child;
    }
}
/** A cancelled consumer stops waiting without aborting a shared first build. */
async function abortable(pending, signal) {
    if (!signal)
        return pending;
    if (signal.aborted)
        throw new Error('native model request cancelled');
    return new Promise((resolve, reject) => {
        const abort = () => { signal.removeEventListener('abort', abort); reject(new Error('native model request cancelled')); };
        signal.addEventListener('abort', abort, { once: true });
        pending.then(value => { signal.removeEventListener('abort', abort); resolve(value); }, error => { signal.removeEventListener('abort', abort); reject(error); });
    });
}
const runtimeRoot = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
const runtimePackage = join(runtimeRoot, 'native', 'warhead-runtime');
const activeBuilds = new Set();
function killBuild(child, signal) {
    if (!child.pid)
        return;
    try {
        process.kill(-child.pid, signal);
    }
    catch {
        child.kill(signal);
    }
}
function runtimeBinaryCandidates() {
    return [
        join(runtimeRoot, 'dist', 'native', 'warhead-runtime'),
        // SwiftPM's native layout, used by current toolchains.
        join(runtimePackage, '.build', 'out', 'Products', 'Release', 'warhead-runtime'),
        // Older/CLI layouts, kept so an existing build is never rebuilt needlessly.
        join(runtimePackage, '.build', 'release', 'warhead-runtime'),
        join(runtimePackage, '.build', 'arm64-apple-macosx', 'release', 'warhead-runtime'),
        join(runtimePackage, '.build', 'x86_64-apple-macosx', 'release', 'warhead-runtime'),
    ];
}
function nativeInputsNewestMtime() {
    const inputs = [join(runtimePackage, 'Package.swift'), join(runtimePackage, 'Package.resolved')];
    const walk = (dir) => {
        if (!existsSync(dir))
            return;
        for (const item of readdirSync(dir, { withFileTypes: true })) {
            const full = join(dir, item.name);
            if (item.isDirectory())
                walk(full);
            else if (item.isFile() && /\.(swift|h|c|cc|cpp|modulemap)$/.test(item.name))
                inputs.push(full);
        }
    };
    walk(join(runtimePackage, 'Sources'));
    return Math.max(0, ...inputs.filter(existsSync).map(path => statSync(path).mtimeMs));
}
function findRuntimeBinary() {
    // Detect edits to ALL native sources, not only the original engine files.
    // Prebuilt distributable binaries are separately verified at packaging.
    const newest = nativeInputsNewestMtime();
    return runtimeBinaryCandidates().find(path => existsSync(path) && statSync(path).isFile() && statSync(path).mtimeMs >= newest) ?? null;
}
let buildPromise = null;
/** Build the Swift runtime once, before any request is issued.
 *
 *  This deliberately does not run under a probe timeout. MLX's first build can
 *  take minutes, and the per-request abort path kills its child to stop token
 *  generation — killing a half-finished SwiftPM build leaves a corrupt `.build`
 *  that fails every later request with missing-artifact errors. The build is
 *  shared by all callers and a failure is retried on the next attempt. */
function ensureRuntimeBuilt() {
    buildPromise ??= buildRuntime().catch(error => {
        buildPromise = null;
        throw error;
    });
    return buildPromise;
}
async function buildRuntime() {
    const existing = findRuntimeBinary();
    if (existing)
        return Promise.resolve(existing);
    if (process.platform !== 'darwin') {
        return Promise.reject(new Error('local MLX and GGUF models require macOS; set WARHEAD_MODEL to an Ollama provider for this host'));
    }
    if (!existsSync(join(runtimePackage, 'Package.swift'))) {
        return Promise.reject(new Error(`native runtime sources are missing: ${runtimePackage}`));
    }
    const swift = await buildCommand('swift', ['--version']);
    if (swift.status !== 0) {
        return Promise.reject(new Error('Swift toolchain not found. Install Xcode or the Swift toolchain, then retry.'));
    }
    // MLX compiles Metal shaders. A missing Metal component fails the build with
    // an opaque "CompileMetalFile failed"; name the fix here instead. `xcrun
    // --find` still succeeds on a broken install, so probe the compiler itself.
    const located = await buildCommand('xcrun', ['--find', 'metal']);
    const metal = located.status === 0 ? located.stdout.trim() : '';
    // xcrun resolves the actual compiler; `metal --version` is not supported
    // consistently by Xcode toolchains and previously caused false failures.
    if (!metal) {
        return Promise.reject(new Error('Xcode\'s Metal Toolchain is not installed. Run:\n  xcodebuild -downloadComponent MetalToolchain'));
    }
    process.stderr.write('building Warhead native runtime (first run only)…\n');
    const build = await buildCommand('swift', ['build', '--configuration', 'release', '-j', '2', '--package-path', runtimePackage], true);
    if (build.status !== 0) {
        return Promise.reject(new Error(`native runtime build failed (exit ${build.status ?? 'unknown'}).\n` +
            `Last compiler output:\n${build.stdout.slice(-4000)}\n` +
            'Check the SwiftPM diagnostics above; for missing Metal components run: xcodebuild -downloadComponent MetalToolchain'));
    }
    const built = findRuntimeBinary();
    if (!built)
        return Promise.reject(new Error('native runtime built successfully but no executable was found'));
    return Promise.resolve(built);
}
function buildCommand(command, args, forward = false) {
    return new Promise(resolve => {
        const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'], detached: true });
        activeBuilds.add(child);
        let stdout = '';
        const timer = setTimeout(() => killBuild(child, 'SIGKILL'), forward ? 10 * 60_000 : 15_000);
        child.stdout.on('data', chunk => { stdout = (stdout + chunk.toString()).slice(-32_768); if (forward)
            process.stderr.write(chunk); });
        child.stderr.on('data', chunk => { stdout = (stdout + chunk.toString()).slice(-32_768); if (forward)
            process.stderr.write(chunk); });
        child.once('error', () => { activeBuilds.delete(child); clearTimeout(timer); resolve({ status: -1, stdout }); });
        child.once('close', status => { activeBuilds.delete(child); clearTimeout(timer); resolve({ status, stdout }); });
    });
}
function toWire(messages) {
    return messages.map(message => {
        if (message.role === 'tool')
            return {
                role: 'tool', content: message.content,
                tool_name: message.toolName ?? 'unknown',
                ...(message.toolCallId ? { tool_call_id: message.toolCallId } : {}),
            };
        if (message.role === 'assistant' && message.toolCalls?.length)
            return {
                role: 'assistant', content: message.content,
                tool_calls: message.toolCalls.map(call => ({ id: call.id, function: { name: call.name, arguments: call.args } })),
            };
        return { role: message.role, content: message.content };
    });
}
const runtime = new NativeRuntime();
export function streamNativeChat(model, messages, opts = {}) {
    return runtime.chat(model, messages, opts.tools ?? [], opts.signal, opts.numPredict, {
        ...(opts.template ? { template: opts.template } : {}),
        ...(opts.stops?.length ? { stops: opts.stops } : {}),
    });
}
/** Build/resolve the native runtime ahead of a caller's timeout window. The
 *  first Swift build can take minutes; a probe timer started before it would
 *  abort the first request immediately after a successful build. */
export const prepareNativeRuntime = () => ensureRuntimeBuilt();
export async function closeNativeRuntime() {
    await Promise.all([runtime.close(), ...Array.from(activeBuilds, child => new Promise(resolve => {
            killBuild(child, 'SIGTERM');
            const timer = setTimeout(() => { killBuild(child, 'SIGKILL'); resolve(); }, 1500);
            child.once('close', () => { clearTimeout(timer); resolve(); });
        }))]);
}
