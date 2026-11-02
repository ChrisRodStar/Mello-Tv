import { chromium } from 'playwright-core';
import { createServer } from 'vite';
import react from '@vitejs/plugin-react';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
const mp4 = await readFile(new URL('./synthetic.mp4', import.meta.url));
const fixtureRoot = dirname(fileURLToPath(import.meta.url));
const server = await createServer({
  configFile: false,
  root: fixtureRoot,
  logLevel: 'error',
  plugins: [react()],
  server: {
    host: '127.0.0.1',
    port: 0,
    hmr: false,
    watch: null,
    fs: { allow: [resolve(fixtureRoot, '../..')] },
  },
});
await server.listen();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
let browser;
try {
  browser = await chromium.launch({
    channel: 'chrome',
    headless: true,
    args: ['--autoplay-policy=no-user-gesture-required'],
  });
} catch (error) {
  await server.close();
  throw error;
}
async function scenario(
  name,
  {
    badEpisodes = [],
    badRefreshes = 0,
    serverFailure = false,
    delayed = false,
    lifecycleFailure = null,
    noSource = false,
  } = {},
  work,
) {
  const ctx = await browser.newContext();
  const errors = [];
  const requests = [];
  const sessions = new Map();
  const keys = new Map();
  let badRemaining = badRefreshes;
  const reply = (s) => ({
    id: s.id,
    title: { type: 'tv', id: 7, season: 1, episode: s.episode },
    url: `/stream/${s.id}/entry?st=${s.bad ? 'bad' : 'good'}${s.revision}`,
    format: 'mp4',
    expires_at: '2099-01-01T00:00:00Z',
    revision: s.revision,
    source_generation: s.revision,
    reload_required: s.revision > 1,
    resume_seconds: 0,
  });
  await ctx.route('**/*', async (route) => {
    const req = route.request(),
      url = new URL(req.url());
    if (url.origin !== base) return route.abort();
    if (url.pathname.startsWith('/stream/')) {
      const id = url.pathname.split('/')[2];
      const s = sessions.get(id);
      requests.push({ kind: 'entry', id, bad: url.search.includes('bad') });
      if (url.search.includes('bad'))
        return route.fulfill({ status: 502, json: { error: 'synthetic upstream 428' } });
      return route.fulfill({
        status: 200,
        contentType: 'video/mp4',
        body: mp4,
        headers: { 'Accept-Ranges': 'bytes' },
      });
    }
    if (!url.pathname.startsWith('/api/')) return route.continue();
    const body = req.postDataJSON();
    const key = req.headers()['idempotency-key'];
    if (req.method() === 'POST' && url.pathname.endsWith('/playback')) {
      let s = keys.get(key);
      if (!s) {
        s = {
          id: `episode-${body.episode}${sessions.has(`episode-${body.episode}`) ? '-restarted' : ''}`,
          episode: body.episode,
          revision: 1,
          bad: badEpisodes.includes(body.episode),
          refreshes: [],
        };
        keys.set(key, s);
        sessions.set(s.id, s);
      }
      requests.push({ kind: 'create', id: s.id });
      return route.fulfill({ json: reply(s) });
    }
    const id = url.pathname.match(/\/playback\/([^/]+)/)?.[1],
      s = sessions.get(id);
    if (req.method() === 'DELETE') {
      requests.push({ kind: 'delete', id });
      return route.fulfill({ status: 204 });
    }
    if (req.method() === 'POST' && url.pathname.endsWith('/refresh')) {
      requests.push({ kind: 'refresh', id, key, body });
      s.refreshes.push(key);
      if (delayed) await new Promise((r) => setTimeout(r, 700));
      if (lifecycleFailure && s.refreshes.length === 1)
        return route.fulfill({ status: 409, json: { error: lifecycleFailure } });
      if (noSource && s.refreshes.length === 1)
        return route.fulfill({ status: 422, json: { error: 'no_source' } });
      if (serverFailure && s.refreshes.length === 1)
        return route
          .fulfill({ status: 500, json: { error: 'synthetic resolver failure' } })
          .catch(() => {});
      s.revision++;
      s.bad = badRemaining-- > 0;
      return route.fulfill({ json: reply(s) }).catch(() => {});
    }
    if (req.method() === 'POST' && url.pathname.endsWith('/backup')) {
      requests.push({ kind: 'backup', id });
      s.revision++;
      s.bad = false;
      return route.fulfill({ json: reply(s) });
    }
    if (req.method() === 'GET' && id) {
      requests.push({ kind: 'get', id });
      return route.fulfill({ json: reply(s) });
    }
    if (req.method() === 'PUT' && url.pathname.endsWith('/progress')) {
      const session = sessions.get(body.playback_id);
      requests.push({
        kind: 'progress',
        id: body.playback_id,
        position: body.position_seconds,
      });
      return route.fulfill({
        json: {
          type: 'tv',
          id: 7,
          season: 1,
          episode: session.episode,
          position_seconds: body.position_seconds,
          duration_seconds: body.duration_seconds,
          watched_seconds: body.watched_seconds,
          completed: false,
          updated_at: '2026-11-02T20:00:00Z',
        },
      });
    }
    return route.fulfill({ status: 404, json: { error: 'unexpected fixture request' } });
  });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(base);
  const playing = async (ep) =>
    page.waitForFunction(
      (ep) => {
        const v = document.querySelector('video');
        return (
          v &&
          v.getAttribute('aria-label') === `Playing Episode ${ep}` &&
          !v.paused &&
          v.currentTime > 0
        );
      },
      ep,
      { timeout: 15000 },
    );
  await playing(1);
  try {
    await work({ page, requests, sessions, playing });
    assert.deepEqual(errors, []);
    console.log('PASS', name);
  } finally {
    await ctx.close();
  }
}
try {
  await scenario(
    'ended episode -> rejected next source -> one fresh lookup -> next plays without leaving',
    { badEpisodes: [2, 3] },
    async ({ page, requests, playing }) => {
      await page.evaluate(() => {
        const v = document.querySelector('video');
        v.currentTime = v.duration - 0.1;
      });
      await playing(2);
      assert.equal(
        requests.filter((r) => r.kind === 'refresh' && r.id === 'episode-2').length,
        1,
      );
      assert.equal(requests.filter((r) => r.kind === 'get' && r.id === 'episode-2').length, 0);
      assert.equal(await page.evaluate(() => window.exits ?? 0), 0);
      assert(
        await page.locator('video').evaluate((v) => v.currentTime < 3),
        'next episode begins at its own start',
      );
      await page.evaluate(() => {
        const v = document.querySelector('video');
        v.currentTime = v.duration - 0.1;
      });
      await playing(3);
      assert.equal(
        requests.filter((r) => r.kind === 'refresh' && r.id === 'episode-3').length,
        1,
        'new episode has independent recovery budget',
      );
      assert.equal(
        requests.filter((r) => r.kind === 'delete' && r.id === 'episode-3').length,
        0,
      );
    },
  );
  await scenario(
    'bounded automatic refresh; manual Retry gets another fresh stream',
    { badEpisodes: [2], badRefreshes: 1 },
    async ({ page, requests, playing }) => {
      await page.getByRole('button', { name: 'Next episode' }).click();
      await page.getByRole('button', { name: 'Try again', exact: true }).waitFor();
      await page.waitForTimeout(500);
      assert.equal(requests.filter((r) => r.kind === 'refresh').length, 1, 'must not loop');
      await page.getByRole('button', { name: 'Try again', exact: true }).click();
      await playing(2);
      assert.equal(requests.filter((r) => r.kind === 'refresh').length, 2);
      assert.equal(
        requests.filter((r) => r.kind === 'get').length,
        0,
        'retry must not reuse broken relay via GET',
      );
    },
  );
  await scenario(
    'mid-episode duplicate errors refresh once and preserve position',
    {},
    async ({ page, requests, playing }) => {
      await page.evaluate(() => {
        const v = document.querySelector('video');
        v.currentTime = 9;
        v.dispatchEvent(new Event('timeupdate'));
        v.dispatchEvent(new Event('error'));
        v.dispatchEvent(new Event('error'));
      });
      await page.waitForFunction(() => document.querySelector('video')?.src.includes('good2'));
      await playing(1);
      assert.equal(requests.filter((r) => r.kind === 'refresh').length, 1);
      assert(
        await page.locator('video').evaluate((v) => v.currentTime >= 9 && v.currentTime < 12),
      );
      assert.equal(
        requests.filter((r) => r.kind === 'delete').length,
        0,
        'refresh keeps progress session',
      );
    },
  );
  await scenario(
    'refresh network failure retry retains idempotency key',
    { badEpisodes: [2], serverFailure: true },
    async ({ page, requests, playing }) => {
      await page.getByRole('button', { name: 'Next episode' }).click();
      await page.getByRole('button', { name: 'Try again', exact: true }).waitFor();
      await page.getByRole('button', { name: 'Try again', exact: true }).click();
      await playing(2);
      const calls = requests.filter((r) => r.kind === 'refresh');
      assert.equal(calls.length, 2);
      assert.equal(calls[0].key, calls[1].key);
    },
  );
  await scenario(
    'changing episodes during refresh ignores stale completion',
    { badEpisodes: [2], delayed: true },
    async ({ page, requests, playing }) => {
      await page.getByRole('button', { name: 'Next episode' }).click();
      const deadline = Date.now() + 10000;
      while (!requests.some((r) => r.kind === 'refresh') && Date.now() < deadline)
        await page.waitForTimeout(20);
      assert(
        requests.some((r) => r.kind === 'refresh'),
        'refresh started before cancellation',
      );
      await page.evaluate(() => window.selectEpisode(3));
      await playing(3);
      await page.waitForTimeout(900);
      await playing(3);
      assert.equal(
        requests.filter((r) => r.kind === 'delete' && r.id === 'episode-3').length,
        0,
      );
      assert.equal(
        await page.getByRole('button', { name: 'Try again', exact: true }).count(),
        0,
      );
    },
  );
  for (const code of ['session_expired', 'revoked']) {
    await scenario(
      `Retry restarts a ${code} session at its saved position`,
      { lifecycleFailure: code },
      async ({ page, requests, playing }) => {
        await page.evaluate(() => {
          const v = document.querySelector('video');
          v.currentTime = 9;
          v.dispatchEvent(new Event('timeupdate'));
          v.dispatchEvent(new Event('error'));
        });
        await page.getByRole('button', { name: 'Try again', exact: true }).waitFor();
        await page.getByRole('button', { name: 'Try again', exact: true }).click();
        await playing(1);
        assert(requests.some((r) => r.kind === 'create' && r.id === 'episode-1-restarted'));
        assert(
          await page.locator('video').evaluate((v) => v.currentTime >= 9 && v.currentTime < 12),
        );
      },
    );
  }
  await scenario(
    'definitive refresh failure does not block a distinct backup',
    { badEpisodes: [2], noSource: true },
    async ({ page, requests, playing }) => {
      await page.getByRole('button', { name: 'Next episode' }).click();
      await page.getByRole('button', { name: 'Try again', exact: true }).waitFor();
      await page.getByRole('button', { name: 'Try backup', exact: true }).click();
      await playing(2);
      assert.equal(requests.filter((r) => r.kind === 'backup').length, 1);
    },
  );
} finally {
  await browser.close();
  await server.close();
}
