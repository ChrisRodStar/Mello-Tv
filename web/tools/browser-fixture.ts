/** Offline browser acceptance: real Go API + built client; synthetic catalog,
 * provider/control boundaries and local generated MP4. No source network/VPN. */
import { mkdtemp, writeFile, rm, chmod } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { generateKeyPairSync, randomBytes, verify } from 'node:crypto';

const root = resolve(import.meta.dir, '..');
const temp = await mkdtemp(join(tmpdir(), 'family-stream-web-fixture-'));
await chmod(temp, 0o700);
const { privateKey, publicKey } = generateKeyPairSync('ed25519');
const token = {
  catalog: 'c'.repeat(32),
  provider: 'p'.repeat(32),
  relay: 'r'.repeat(32),
  setup: 'b'.repeat(32),
};
let closing = false;
let child: ReturnType<typeof Bun.spawn> | undefined;
const servers: ReturnType<typeof Bun.serve>[] = [];
async function close() {
  if (closing) return;
  closing = true;
  if (child) {
    child.kill('SIGTERM');
    await Promise.race([child.exited, new Promise((resolve) => setTimeout(resolve, 5000))]);
    if (child.exitCode === null) {
      child.kill('SIGKILL');
      await child.exited;
    }
  }
  for (const s of servers) s.stop(true);
  await rm(temp, { recursive: true, force: true });
  process.exit(0);
}
process.on('SIGTERM', () => void close());
process.on('SIGINT', () => void close());
setTimeout(() => void close(), 15 * 60 * 1000).unref();
function serve(fetch: Parameters<typeof Bun.serve>[0]['fetch']) {
  const server = Bun.serve({ hostname: '127.0.0.1', port: 0, fetch });
  servers.push(server);
  return server;
}
const json = (v: unknown, status = 200) => Response.json(v, { status });
const now = () => new Date().toISOString();
let catalogDown = false;
const title = (type: 'movie' | 'tv', id: number, name: string) => ({
  type,
  id,
  title: name,
  overview: 'Synthetic catalog content for local browser acceptance.',
  date: '2026-01-01',
  rating: 8,
  poster: '/api/v1/artwork/w500/synthetic.png',
  backdrop: '/api/v1/artwork/w1280/synthetic.png',
  runtime: 20,
  genres: ['action', 'comedy', 'drama'],
  details: true,
  ...(type === 'tv' ? { seasons: [{ season: 0, name: 'Specials', episode_count: 2 }] } : {}),
});
const movies = [
  title('movie', 7, 'Synthetic Movie'),
  title('movie', 8, 'Synthetic Sequel'),
  title('tv', 9, 'Synthetic Series'),
];
let revision = 1;
const started = now();
const expiry = new Date(Date.now() + 86400000).toISOString();
const rec = (kind: string, key: string, data: unknown) => ({
  kind,
  key,
  data,
  updated_at: started,
  expires_at: expiry,
  revision: 1,
  state: 'fresh',
});
const shelf = { id: 'trending', heading: 'Trending this week', titles: movies };
const records = [
  ...movies.map((t) =>
    rec('title', `${t.type}:${t.id}`, {
      ...t,
      poster: t.poster.replace('/api/v1/', '/internal/v1/'),
      backdrop: t.backdrop.replace('/api/v1/', '/internal/v1/'),
    }),
  ),
  rec('shelf', 'trending', {
    ...shelf,
    titles: movies.map((t) => ({
      ...t,
      poster: t.poster.replace('/api/v1/', '/internal/v1/'),
      backdrop: t.backdrop.replace('/api/v1/', '/internal/v1/'),
    })),
  }),
  rec('season', 'tv:9:0', {
    show_id: 9,
    season: 0,
    name: 'Specials',
    episodes: [1, 2].map((n) => ({
      tmdb_episode_id: n,
      season: 0,
      episode: n,
      name: `Special ${n}`,
      overview: 'Synthetic episode.',
      runtime: 20,
    })),
  }),
];
const image = Buffer.from(
  await Bun.file(join(root, '.local/synthetic.png')).arrayBuffer(),
).toString('base64');
records.push(
  rec('artwork', 'w500/synthetic.png', { mime: 'image/png', bytes: image }),
  rec('artwork', 'w1280/synthetic.png', { mime: 'image/png', bytes: image }),
);
const recommendations = new Map<string, unknown>();
const catalog = serve(async (request) => {
  if (request.headers.get('authorization') !== `Bearer ${token.catalog}`)
    return json({ error: 'unauthorized' }, 401);
  if (catalogDown) return json({ error: 'unavailable' }, 503);
  const url = new URL(request.url);
  if (url.pathname === '/internal/v1/snapshot')
    return json({
      version: '1',
      revision,
      exported_at: now(),
      records: [...records, ...recommendations.values()],
    });
  if (request.method === 'PUT' && url.pathname.endsWith('/inputs')) {
    const pid = url.pathname.split('/')[4]!;
    const input = await request.json();
    if (input.completed) {
      recommendations.set(
        pid,
        rec('recommendations', pid, {
          fingerprint: 'synthetic',
          pool: movies,
          sections: [{ id: 'for-you', heading: 'For you', titles: movies }],
        }),
      );
      revision++;
    }
    return json({ status: 'queued' }, 202);
  }
  if (request.method === 'DELETE') {
    recommendations.delete(url.pathname.split('/')[4]!);
    revision++;
    return new Response(null, { status: 204 });
  }
  return json({ status: 'queued' }, 202);
});
let resolves = 0;
const provider = serve(async (request) => {
  if (request.headers.get('authorization') !== `Bearer ${token.provider}`)
    return json({ error: 'unauthorized' }, 401);
  resolves++;
  const requestBody = await request.json();
  const ranked = {
    candidate: {
      source: {
        id: 'synthetic-source',
        provider: 'synthetic',
        title: requestBody.lookup.title,
        url: 'http://synthetic.invalid/movie.mp4',
      },
      observation: { format: 'mp4' },
    },
    score: 1,
    reasons: [],
  };
  return json({
    version: 'providers.v1',
    selected: ranked,
    backups: [ranked],
    rejected: {},
    providers: [],
  });
});
type Session = {
  session_id: string;
  contract_version: string;
  entry_path: string;
  format: string;
  expires_at: string;
  revision: number;
  auth_revision: number;
  source_generation: number;
  state: string;
  resume_required?: boolean;
};
const sessions = new Map<string, Session>();
const creates = new Map<string, string>();
const actions: string[] = [];
const relay = serve(async (request) => {
  if (request.headers.get('authorization') !== `Bearer ${token.relay}`)
    return json({ code: 'unauthorized' }, 401);
  const url = new URL(request.url);
  const parts = url.pathname.split('/');
  const sid = parts[4];
  if (request.method === 'POST' && url.pathname === '/internal/v1/sessions') {
    const key = request.headers.get('idempotency-key')!;
    const saved = creates.get(key);
    if (saved) return json(sessions.get(saved));
    const body = await request.json();
    const id = randomBytes(18).toString('base64url');
    const state: Session = {
      contract_version: 'relay.v1',
      session_id: id,
      entry_path: `/stream/${id}/entry`,
      format: 'mp4',
      expires_at: body.expires_at,
      revision: 1,
      auth_revision: 11,
      source_generation: 1,
      state: 'active',
    };
    sessions.set(id, state);
    creates.set(key, id);
    actions.push('create');
    return json(state, 201);
  }
  const state = sessions.get(sid!);
  if (!state) return json({ code: 'unknown_resource' }, 404);
  if (request.method === 'DELETE') {
    state.state = 'revoked';
    state.auth_revision++;
    actions.push('revoke');
    return new Response(null, { status: 204 });
  }
  if (request.method === 'GET') return json(state);
  const body = await request.json();
  if (body.expected_revision !== state.revision)
    return json({ code: 'revision_conflict' }, 409);
  state.revision++;
  const kind = parts[5]!;
  if (kind === 'renew') state.expires_at = body.expires_at;
  else {
    state.source_generation++;
    state.resume_required = true;
  }
  actions.push(kind);
  return json(state);
});
const probe = serve(() => new Response());
const apiPort = probe.port!;
probe.stop(true);
let apiOrigin = `http://127.0.0.1:${apiPort}`;
const media = Bun.file(join(root, '.local/synthetic.mp4'));
if (!(await media.exists())) throw new Error('Generate the local synthetic video first');
const bytes = new Uint8Array(await media.arrayBuffer());
const front = serve(async (request) => {
  const url = new URL(request.url);
  if (url.pathname === '/__fixture/report')
    return json({ resolves, actions, session_count: sessions.size, catalog_down: catalogDown });
  if (url.pathname === '/__fixture/catalog/down' && request.method === 'POST') {
    catalogDown = true;
    return json({ ok: true });
  }
  if (url.pathname.startsWith('/api/v1/')) {
    try {
      const result = await fetch(new Request(apiOrigin + url.pathname + url.search, request), {
        redirect: 'manual',
      });
      return new Response(result.body, { status: result.status, headers: result.headers });
    } catch {
      return json({ error: 'api_unavailable' }, 503);
    }
  }
  if (url.pathname.startsWith('/stream/')) {
    const sid = url.pathname.split('/')[2]!;
    const s = sessions.get(sid);
    const st = url.searchParams.get('st') ?? '';
    const pieces = st.split('.');
    let claims: any;
    try {
      claims = JSON.parse(Buffer.from(pieces[1]!, 'base64url').toString());
      if (
        !verify(
          null,
          Buffer.from(`${pieces[0]}.${pieces[1]}`),
          publicKey,
          Buffer.from(pieces[2]!, 'base64url'),
        )
      )
        throw new Error();
    } catch {
      return json({ code: 'invalid_grant' }, 401);
    }
    if (
      !s ||
      s.state !== 'active' ||
      claims.sid !== sid ||
      claims.arv !== s.auth_revision ||
      claims.exp < Date.now() / 1000
    )
      return json({ code: 'revoked_grant' }, 403);
    const headers: Record<string, string> = {
      'Content-Type': 'video/mp4',
      'Accept-Ranges': 'bytes',
      'Cache-Control': 'private, no-store',
    };
    let start = 0,
      end = bytes.length - 1,
      status = 200;
    const range = request.headers.get('range');
    if (range) {
      const match = /^bytes=(\d+)-(\d*)$/.exec(range);
      if (!match) return new Response(null, { status: 416 });
      start = Number(match[1]);
      end = match[2] ? Math.min(Number(match[2]), end) : end;
      if (start > end) return new Response(null, { status: 416 });
      status = 206;
      headers['Content-Range'] = `bytes ${start}-${end}/${bytes.length}`;
    }
    headers['Content-Length'] = String(end - start + 1);
    return new Response(request.method === 'HEAD' ? null : bytes.slice(start, end + 1), {
      status,
      headers,
    });
  }
  if (url.pathname.startsWith('/internal/')) return new Response(null, { status: 404 });
  if (url.pathname.startsWith('/api/')) return json({ error: 'legacy_route_rejected' }, 404);
  const path = resolve(root, 'dist', '.' + decodeURIComponent(url.pathname));
  if (!path.startsWith(resolve(root, 'dist') + '/')) {
    return new Response(Bun.file(join(root, 'dist/index.html')));
  }
  const file = Bun.file(path);
  return new Response((await file.exists()) ? file : Bun.file(join(root, 'dist/index.html')));
});
async function secret(name: string, data: Uint8Array | string) {
  const path = join(temp, name);
  await writeFile(path, data, { mode: 0o600 });
  return path;
}
const config = {
  listen: `127.0.0.1:${apiPort}`,
  origin: `http://127.0.0.1:${front.port}`,
  db_path: join(temp, 'api.sqlite'),
  catalog_url: `http://127.0.0.1:${catalog.port}`,
  provider_url: `http://127.0.0.1:${provider.port}`,
  relay_url: `http://127.0.0.1:${relay.port}`,
  kid: 'browser-test',
  catalog_token_file: await secret('catalog', token.catalog),
  provider_token_file: await secret('provider', token.provider),
  relay_token_file: await secret('relay', token.relay),
  setup_token_file: await secret('setup', token.setup),
  encryption_key_file: await secret('encryption', randomBytes(32)),
  signing_key_file: await secret('signer', privateKey.export({ format: 'der', type: 'pkcs8' })),
};
const configPath = await secret('api.json', JSON.stringify(config));
child = Bun.spawn([resolve(root, '../family-stream-api/.local/api'), configPath], {
  env: { PATH: process.env.PATH! },
  stdout: 'ignore',
  stderr: 'ignore',
});
for (let i = 0; i < 100; i++) {
  try {
    const r = await fetch(apiOrigin + '/health/readyz');
    if (r.ok) break;
  } catch {}
  if (i === 99) throw new Error('API startup unavailable');
  await Bun.sleep(50);
}
await writeFile(
  join(root, '.local/browser-fixture-ready.json'),
  JSON.stringify({ origin: `http://127.0.0.1:${front.port}` }),
  { mode: 0o600 },
);
console.log('Synthetic browser fixture ready');
