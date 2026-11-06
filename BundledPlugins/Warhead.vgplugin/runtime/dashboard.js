// src/dashboard.ts
// Simple HTTP server that displays the current goal ledger and queue.
// No external dependencies – uses Node's built‑in http module.
import { createServer, IncomingMessage, ServerResponse } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { homedir } from 'node:os';
const PORT = process.env['WARHEAD_DASHBOARD_PORT'] ? parseInt(process.env['WARHEAD_DASHBOARD_PORT']) : 3001;
const BASE_DIR = join(homedir(), '.local', 'share', 'warhead');
const GOALS_PATH = join(BASE_DIR, 'goals.json');
const QUEUE_PATH = join(BASE_DIR, 'queue.json');
function jsonResponse(res, data) {
    const payload = JSON.stringify(data, null, 2);
    res.writeHead(200, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
    res.end(payload);
}
function notFound(res) {
    res.writeHead(404, { 'Content-Type': 'text/plain' });
    res.end('Not found');
}
function serveIndex(res) {
    const html = `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Warhead Dashboard</title>
  <style>
    body { font-family: sans-serif; margin: 2rem; }
    h1 { margin-bottom: 0.5rem; }
    pre { background: #f4f4f4; padding: 1rem; overflow-x: auto; }
    button { margin-top: 1rem; }
  </style>
</head>
<body>
  <h1>Warhead Goal Ledger</h1>
  <pre id="goals">Loading…</pre>
  <h2>Goal Queue</h2>
  <pre id="queue">Loading…</pre>
  <script>
    async function load(path, el) {
      try {
        const r = await fetch(path);
        const data = await r.json();
        el.textContent = JSON.stringify(data, null, 2);
      } catch (e) {
        el.textContent = 'Error: ' + e;
      }
    }
    load('/goals', document.getElementById('goals'));
    load('/queue', document.getElementById('queue'));
    // Auto‑refresh every 5 seconds
    setInterval(() => {
      load('/goals', document.getElementById('goals'));
      load('/queue', document.getElementById('queue'));
    }, 5000);
  </script>
</body>
</html>`;
    res.writeHead(200, { 'Content-Type': 'text/html' });
    res.end(html);
}
const server = createServer((req, res) => {
    const url = req.url ?? '/';
    if (url === '/' || url === '/index.html') {
        serveIndex(res);
    }
    else if (url === '/goals') {
        const data = existsSync(GOALS_PATH) ? JSON.parse(readFileSync(GOALS_PATH, 'utf8')) : [];
        jsonResponse(res, data);
    }
    else if (url === '/queue') {
        const data = existsSync(QUEUE_PATH) ? JSON.parse(readFileSync(QUEUE_PATH, 'utf8')) : [];
        jsonResponse(res, data);
    }
    else {
        notFound(res);
    }
});
server.listen(PORT, () => {
    console.log(`Warhead dashboard listening on http://localhost:${PORT}`);
});
