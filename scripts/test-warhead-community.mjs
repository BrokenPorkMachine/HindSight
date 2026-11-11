#!/usr/bin/env node
/**
 * Warhead Community release-policy regression tests.
 * Run: node --test scripts/test-warhead-community.mjs
 */
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Registry } from '../BundledPlugins/Warhead.vgplugin/runtime/tools/registry.js';
import { createTodoStore } from '../BundledPlugins/Warhead.vgplugin/runtime/tools/todo.js';
import { sensitiveCommunityPath } from '../BundledPlugins/Warhead.vgplugin/runtime/community/guardrails.js';

const makeCtx = root => ({
  cwd: root,
  workspaceRoot: root,
  communitySnapshot: false,
  signal: new AbortController().signal,
  todos: createTodoStore(),
  config: () => ({ limits: {} }),
});

test('community registry does not advertise shell, writes, web, or external tools', () => {
  const reg = new Registry();
  const names = reg.names();
  assert.deepEqual(names.sort(), ['read_file', 'todo', 'hindsight_context'].sort());
  assert.deepEqual(reg.definitions().map(x => x.name).sort(), names.sort());
});

test('tool dispatch denies shell, file changes, network and unknown tools', async () => {
  const reg = new Registry();
  const ctx = makeCtx(process.cwd());
  for (const name of ['bash', 'write', 'edit', 'multi_edit', 'web_search',
      'fetch_web_page', 'mcp_call_tool', 'rea_run_task', 'unknown']) {
    const out = await reg.dispatch(name, { command: 'echo unsafe', path: 'x' }, ctx);
    assert.equal(out.ok, false, name);
    assert.equal(out.error, 'community policy denied', name);
  }
});

test('dynamic tool registration cannot extend community authority', async () => {
  let called = false;
  const reg = new Registry();
  reg.register({
    mutates: false,
    def: { name: 'dangerous_custom', description: 'simulated external tool', parameters: { type: 'object' } },
    async run() { called = true; return { ok: true, output: 'executed' }; },
  });
  assert.equal(reg.names().includes('dangerous_custom'), false);
  const result = await reg.dispatch('dangerous_custom', {}, makeCtx(process.cwd()));
  assert.equal(result.ok, false);
  assert.equal(called, false);
});

test('HindSight file export cannot access a path without Studio snapshot authority', async () => {
  const reg = new Registry();
  assert.equal((await reg.dispatch('hindsight_context', { path: '/etc/hosts' }, makeCtx(process.cwd()))).ok, false);
});

test('sensitive workspace files are excluded', async () => {
  const root = mkdtempSync(join(tmpdir(), 'warhead-community-'));
  try {
    writeFileSync(join(root, 'ok.txt'), 'safe text\n');
    writeFileSync(join(root, '.env'), 'NOT_FOR_AGENT=secret\n');
    mkdirSync(join(root, '.ssh'));
    writeFileSync(join(root, '.ssh', 'id_rsa'), 'NOT_FOR_AGENT\n');
    const reg = new Registry();
    const ctx = makeCtx(root);
    const safe = await reg.dispatch('read_file', { path: 'ok.txt', limit: 10 }, ctx);
    assert.equal(safe.ok, true);
    assert.match(safe.output, /safe text/);
    symlinkSync('/etc/hosts', join(root, 'linked-hosts'));
    for (const path of ['.env', '.ssh/id_rsa', '../out-of-bounds', '/etc/hosts', 'linked-hosts']) {
      const result = await reg.dispatch('read_file', { path }, ctx);
      assert.equal(result.ok, false, path);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('fail closed on absent workspace authority, even with nominally safe read', async () => {
  const reg = new Registry();
  const ctx = makeCtx(process.cwd());
  delete ctx.workspaceRoot;
  const out = await reg.dispatch('read_file', { path: 'README.md' }, ctx);
  assert.equal(out.ok, false);
  assert.equal(out.error, 'community policy denied');
});

test('hidden credentials variants are marked sensitive', () => {
  for (const path of ['.env', './.env.local', '.ssh/id_ed25519', 'config/cert.pem', '.aws/credentials', 'user.key']) {
    assert.equal(sensitiveCommunityPath(path), true);
  }
});

test('todo remains session-local and usable', async () => {
  const reg = new Registry();
  const result = await reg.dispatch('todo', { todos: [{ content: 'Inspect selected evidence', status: 'pending' }] }, makeCtx(process.cwd()));
  assert.equal(result.ok, true);
});
