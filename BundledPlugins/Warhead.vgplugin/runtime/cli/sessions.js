import { dirname, join } from 'node:path';
import { chmodSync, existsSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import { configPath } from '../config.js';
import { parseSession } from '../context/session.js';
import { writePrivateJson, writePrivateText } from '../context/storage.js';
import { DIM, GREEN, out, RESET } from './style.js';
function sessionPath(name) {
    const safe = name.trim().replace(/[^a-zA-Z0-9._-]+/g, '-').replace(/^-|-$/g, '').slice(0, 80) || 'session';
    return join(dirname(configPath()), 'sessions', `${safe}.json`);
}
export async function sessionCommand(command, name, state, context) {
    if (!name) {
        out(`usage: /${command} <${command === 'export' ? 'file.md' : 'name'}>`);
        return null;
    }
    if (command === 'load') {
        const path = sessionPath(name);
        const stat = statSync(path);
        if (!stat.isFile() || stat.size > 32 * 1024 * 1024)
            throw new Error('Session must be a regular file of at most 32 MiB');
        const saved = parseSession(JSON.parse(readFileSync(path, 'utf8')));
        out(`${GREEN}loaded${RESET} ${path} ${DIM}(${saved.history.length} messages)${RESET}`);
        return saved;
    }
    if (command === 'save') {
        const path = sessionPath(name);
        mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
        chmodSync(dirname(path), 0o700);
        writePrivateJson(path, { version: 2, cwd: context.cwd, model: context.model, savedAt: new Date().toISOString(), ...state });
        out(`${GREEN}saved${RESET} ${path} ${DIM}(${state.history.length} messages)${RESET}`);
        return null;
    }
    if (existsSync(name)) {
        const answer = await context.askLine(`overwrite ${name}? [y/N] `);
        if (answer === null || !/^y(es)?$/i.test(answer.trim())) {
            out('export cancelled');
            return null;
        }
    }
    const markdown = state.history.map(message => {
        const title = message.role === 'user' ? 'User' : message.role === 'assistant' ? 'Warhead' : `Tool: ${message.toolName ?? 'result'}`;
        return `## ${title}\n\n${message.content}`;
    }).join('\n\n') + '\n';
    // Export contains conversation data and uses the same durable private writer.
    writePrivateText(name, markdown);
    out(`${GREEN}exported${RESET} ${name}`);
    return null;
}
