import { parentPort, workerData } from 'node:worker_threads';
import { grep } from './grep.js';
import { glob } from './glob.js';
import { createTodoStore } from './todo.js';
import { DEFAULT_CONFIG } from '../config.js';
if (parentPort) {
    const { name, args, cwd } = workerData;
    const result = await (name === 'grep' ? grep : glob).run(args, {
        cwd, signal: new AbortController().signal, todos: createTodoStore(), config: () => DEFAULT_CONFIG,
    });
    parentPort.postMessage(result);
}
