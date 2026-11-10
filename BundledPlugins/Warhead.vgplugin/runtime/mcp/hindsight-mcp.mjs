#!/usr/bin/env node
/**
 * HindSight MCP server.
 *
 * This is deliberately a small, dependency-free stdio server.  It translates
 * MCP tools to the authenticated HTTPS API exposed by a running HindSight app;
 * it never opens project files or executes imported binaries itself.
 */
import fs from 'node:fs';
import http from 'node:http';
import https from 'node:https';
import readline from 'node:readline';

const VERSION = '0.1.0';
const MAX_RESPONSE_BYTES = 16 * 1024 * 1024;

function configuration() {
  const base = process.env.HINDSIGHT_URL?.trim();
  const token = process.env.HINDSIGHT_TOKEN?.trim();
  if (!base) throw new Error('HINDSIGHT_URL is required (for example https://127.0.0.1:8443/v1)');
  if (!token) throw new Error('HINDSIGHT_TOKEN is required (copy the gateway token from HindSight)');
  const url = new URL(base.endsWith('/') ? base : `${base}/`);
  if (url.protocol !== 'https:' && !['localhost', '127.0.0.1', '::1'].includes(url.hostname)) {
    throw new Error('HINDSIGHT_URL must use HTTPS unless it targets localhost');
  }
  let ca;
  const caFile = process.env.HINDSIGHT_CA_FILE?.trim();
  const caPEM = process.env.HINDSIGHT_CA_PEM?.trim();
  if (caPEM) ca = caPEM;
  else if (caFile) ca = fs.readFileSync(caFile);
  const certificateSHA256 = process.env.HINDSIGHT_CERT_SHA256?.trim().toLowerCase();
  if (certificateSHA256 && !/^[a-f0-9]{64}$/.test(certificateSHA256)) throw new Error('HINDSIGHT_CERT_SHA256 is invalid');
  return { url, token, ca, certificateSHA256 };
}

function apiRequest(method, path, body) {
    const { url: base, token, ca, certificateSHA256 } = configuration();
  const url = new URL(path.replace(/^\//, ''), base);
  const payload = body === undefined ? undefined : JSON.stringify(body);
  const transport = url.protocol === 'https:' ? https : http;
  return new Promise((resolve, reject) => {
    const request = transport.request(url, {
      method,
      ca,
      ...(certificateSHA256 ? { checkServerIdentity: (_host, certificate) => {
        const actual = certificate.fingerprint256?.replaceAll(':', '').toLowerCase();
        return actual === certificateSHA256 ? undefined : new Error('HindSight TLS certificate fingerprint mismatch');
      } } : {}),
      headers: {
        accept: 'application/json',
        authorization: `Bearer ${token}`,
        ...(payload ? { 'content-type': 'application/json', 'content-length': Buffer.byteLength(payload) } : {}),
      },
    }, response => {
      const chunks = [];
      let size = 0;
      response.on('data', chunk => {
        size += chunk.length;
        if (size > MAX_RESPONSE_BYTES) {
          request.destroy(new Error('HindSight response exceeded 16 MiB'));
        } else chunks.push(chunk);
      });
      response.on('end', () => {
        const text = Buffer.concat(chunks).toString('utf8');
        let data;
        try { data = text === '' ? null : JSON.parse(text); } catch { data = { raw: text }; }
        if ((response.statusCode ?? 500) >= 400) {
          const message = typeof data?.error === 'string' ? data.error : `HindSight returned HTTP ${response.statusCode}`;
          reject(new Error(message));
          return;
        }
        resolve(data);
      });
    });
    request.setTimeout(125_000, () => request.destroy(new Error('HindSight request timed out')));
    request.on('error', reject);
    if (payload) request.write(payload);
    request.end();
  });
}

const schemas = {
  empty: { type: 'object', additionalProperties: false },
  submit: {
    type: 'object', additionalProperties: false,
    properties: {
      request_id: { type: 'string', description: 'A fresh UUID. Reuse only to retry the same source-bound request.' },
      project_path: { type: 'string', description: 'Exact path returned by hindsight_project.' },
      record_id: { type: 'string', description: 'Record UUID returned by hindsight_project.' },
      source_sha256: { type: 'string', description: 'Exact captured source SHA-256 returned by hindsight_project.' },
    }, required: ['request_id', 'project_path', 'record_id', 'source_sha256'],
  },
  job: { type: 'object', additionalProperties: false, properties: { job_id: { type: 'string' } }, required: ['job_id'] },
};

const tools = [
  { name: 'hindsight_capabilities', description: 'Discover the exact capabilities granted by the connected HindSight gateway. Call this before choosing another tool.', inputSchema: schemas.empty, run: () => apiRequest('GET', 'capabilities') },
  { name: 'hindsight_project', description: 'Read the currently open, explicitly granted HindSight project. Includes source-bound artifact record IDs and SHA-256 identities.', inputSchema: schemas.empty, run: () => apiRequest('GET', 'project') },
  { name: 'hindsight_analysis_submit', description: 'Submit an asynchronous static-analysis job for a captured project record. This requests analysis only; it does not execute the imported binary.', inputSchema: schemas.submit, run: a => apiRequest('POST', 'jobs/analysis', a) },
  { name: 'hindsight_analysis_status', description: 'Poll a previously submitted analysis job for its state and progress.', inputSchema: schemas.job, run: a => apiRequest('GET', `jobs/${encodeURIComponent(a.job_id)}`) },
  { name: 'hindsight_analysis_result', description: 'Read the source-bound result of a completed analysis job.', inputSchema: schemas.job, run: a => apiRequest('GET', `jobs/${encodeURIComponent(a.job_id)}/result`) },
  { name: 'hindsight_analysis_cancel', description: 'Request cancellation of a non-terminal analysis job.', inputSchema: schemas.job, run: a => apiRequest('DELETE', `jobs/${encodeURIComponent(a.job_id)}`) },
];

function toolResult(value, isError = false) {
  return { content: [{ type: 'text', text: JSON.stringify(value, null, 2) }], ...(isError ? { isError: true } : {}) };
}

async function handle(message) {
  const { id, method, params = {} } = message;
  if (method === 'notifications/initialized' || method === 'notifications/cancelled' || method === 'exit') return null;
  if (method === 'initialize') return { jsonrpc: '2.0', id, result: { protocolVersion: '2025-03-26', capabilities: { tools: {} }, serverInfo: { name: 'hindsight', version: VERSION } } };
  if (method === 'tools/list') return { jsonrpc: '2.0', id, result: { tools: tools.map(({ run, ...tool }) => tool) } };
  if (method === 'tools/call') {
    const tool = tools.find(item => item.name === params.name);
    if (!tool) return { jsonrpc: '2.0', id, result: toolResult({ error: `Unknown tool: ${params.name}` }, true) };
    try { return { jsonrpc: '2.0', id, result: toolResult(await tool.run(params.arguments ?? {})) }; }
    catch (error) { return { jsonrpc: '2.0', id, result: toolResult({ error: error instanceof Error ? error.message : String(error) }, true) }; }
  }
  return { jsonrpc: '2.0', id, error: { code: -32601, message: `Method not found: ${method}` } };
}

if (process.argv.includes('--help')) {
  console.log('Usage: HINDSIGHT_URL=https://host:8443/v1 HINDSIGHT_TOKEN=... node hindsight-mcp.mjs');
  process.exit(0);
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of input) {
  try {
    const response = await handle(JSON.parse(line));
    if (response) process.stdout.write(`${JSON.stringify(response)}\n`);
  } catch (error) {
    process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', id: null, error: { code: -32700, message: error instanceof Error ? error.message : 'Invalid JSON-RPC request' } })}\n`);
  }
}
