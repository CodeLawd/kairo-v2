#!/usr/bin/env node
/**
 * Serves the Electron checkout's real audience Worker (`server/cloudflare/
 * audience-routes.ts` and the `AudienceRoom` Durable Object) on a local
 * port, through Miniflare, for prompt 046's live tests. Account checks are
 * answered locally as in `scripts/test-engage-room.mjs`: `Bearer device`
 * is an activated device of org `org`. No deployed service or secrets.
 *
 * Prints `READY <base url>` and serves until stdin closes.
 * `GET /__invites` returns the moderator invites the Worker sent.
 */
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { createServer } from 'node:http'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

const electron = resolve(process.env.KAIRO_ELECTRON_DIR || fileURLToPath(new URL('../../../kairo-presenter', import.meta.url)))
const require = createRequire(join(electron, 'server/package.json'))
const { Miniflare, convertV4MiniflareOptions } = require('miniflare')
const { build } = require('esbuild')

const dir = await mkdtemp(join(tmpdir(), 'kairo-audience-worker-'))
const bundle = join(dir, 'worker.mjs')
const invites = []
let mf
let internal
const shutdown = async () => {
  await mf?.dispose().catch(() => {})
  await new Promise(r => (internal ? internal.close(r) : r()))
  await rm(dir, { recursive: true, force: true })
  process.exit(0)
}
try {
  await build({
    stdin: {
      contents: `import { audienceRoutes } from './server/cloudflare/audience-routes';
        export { AudienceRoom } from './server/cloudflare/audience-room';
        export default { async fetch(request, env) {
          if (new URL(request.url).pathname === '/__invites') return fetch(env.LOCAL_API_URL + '/__invites');
          return await audienceRoutes(request, env, async request => {
            const path = new URL(request.url).pathname;
            if (path === '/v1/licensing') return Response.json({ activated: request.headers.get('Authorization') === 'Bearer device', lease: { claims: { orgId: 'org' } } });
            if (path === '/v1/orgs/org/engagements' || path.startsWith('/v1/internal/')) return fetch(env.LOCAL_API_URL + path, { method: request.method, headers: request.headers, body: request.method === 'GET' ? undefined : await request.text() });
            return Response.json({ name: 'Test church' });
          }) || new Response('Not found', { status: 404 });
        }};`,
      resolveDir: electron,
      sourcefile: 'audience-dev-worker.ts',
    },
    outfile: bundle, bundle: true, format: 'esm', platform: 'neutral', external: ['cloudflare:workers'], logLevel: 'silent',
  })
  // The account server's engagement index and invite mailbox.
  const index = new Map()
  internal = createServer(async (req, res) => {
    let text = ''
    for await (const chunk of req) text += chunk
    res.setHeader('Content-Type', 'application/json')
    if (req.url === '/__invites') { res.end(JSON.stringify(invites)); return }
    if (req.url === '/v1/orgs/org/engagements') { res.end(JSON.stringify([...index.values()])); return }
    const body = text ? JSON.parse(text) : {}
    if (req.url.endsWith('/invite')) invites.push(body)
    else if (req.url.endsWith('/engage/index')) {
      if (body.deleted) index.delete(body.id)
      else index.set(body.id, { id: body.id, code: body.code, name: body.name, status: body.status, startsAt: body.startsAt ?? null, endsAt: body.endsAt ?? null })
    }
    res.end('{}')
  })
  await new Promise(r => internal.listen(0, '127.0.0.1', r))
  const local = `http://127.0.0.1:${internal.address().port}`
  mf = new Miniflare(convertV4MiniflareOptions({
    host: '127.0.0.1', port: 0, name: 'audience-dev', modulesRoot: dir, modules: true, scriptPath: bundle, compatibilityDate: '2026-10-24',
    durableObjects: { AUDIENCE_ROOM: { className: 'AudienceRoom', useSQLite: true } },
    bindings: { LOCAL_API_URL: local, ENGAGE_INTERNAL_SECRET: 'test-secret', WEB_ORIGIN: 'https://kairopresenter.com', PUBLIC_WEB_URL: 'https://kairopresenter.com' },
  }))
  const url = String(await mf.ready).replace(/\/$/, '')
  console.log(`READY ${url}`)
  process.stdin.resume()
  process.stdin.on('end', shutdown)
  process.stdin.on('close', shutdown)
  process.on('SIGTERM', shutdown)
} catch (error) {
  console.error(error)
  await shutdown()
}
