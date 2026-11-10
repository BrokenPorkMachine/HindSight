#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const server = resolve(root, 'mcp/hindsight-mcp.mjs');

const child = spawn(process.execPath, [server], {
  cwd: root,
  env: { ...process.env, HINDSIGHT_URL: '', HINDSIGHT_TOKEN: '' },
  stdio: ['pipe', 'pipe', 'pipe'],
});

let stdout = '';
let stderr = '';
child.stdout.setEncoding('utf8').on('data', chunk => { stdout += chunk; });
child.stderr.setEncoding('utf8').on('data', chunk => { stderr += chunk; });

const requests = [
  { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'test', version: '1' } } },
  { jsonrpc: '2.0', method: 'notifications/initialized' },
  { jsonrpc: '2.0', id: 2, method: 'tools/list', params: {} },
];

for (const request of requests) child.stdin.write(`${JSON.stringify(request)}\n`);

const timer = setTimeout(() => {
  child.kill('SIGKILL');
  console.error('HindSight MCP protocol test timed out');
  if (stderr) console.error(stderr.trim());
  process.exit(1);
}, 5000);

child.on('error', error => {
  clearTimeout(timer);
  console.error(error.message);
  process.exit(1);
});

child.on('close', code => {
  clearTimeout(timer);
  const lines = stdout.trim().split('\n').filter(Boolean);
  let messages;
  try {
    messages = lines.map(line => JSON.parse(line));
  } catch (error) {
    console.error('Invalid MCP JSON output');
    if (stderr) console.error(stderr.trim());
    process.exit(1);
  }
  const init = messages.find(message => message.id === 1);
  const list = messages.find(message => message.id === 2);
  const names = (list?.result?.tools ?? []).map(tool => tool.name).sort();
  const expected = [
    'hindsight_analysis_cancel',
    'hindsight_analysis_result',
    'hindsight_analysis_status',
    'hindsight_analysis_submit',
    'hindsight_capabilities',
    'hindsight_project',
  ];
  if (code !== 0 && code !== null) {
    console.error(`MCP server exited with code ${code}`);
    if (stderr) console.error(stderr.trim());
    process.exit(1);
  }
  if (init?.result?.protocolVersion !== '2025-03-26') {
    console.error('Missing or invalid initialize response');
    process.exit(1);
  }
  if (JSON.stringify(names) !== JSON.stringify(expected)) {
    console.error(`Unexpected tools: ${JSON.stringify(names)}`);
    process.exit(1);
  }
  if (stderr.trim()) {
    console.error(`Unexpected stderr: ${stderr.trim()}`);
    process.exit(1);
  }
  console.log('HindSight MCP protocol contract passed.');
});

child.stdin.end();
