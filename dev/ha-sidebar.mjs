#!/usr/bin/env node
// Adds ergo to the dev Home Assistant's sidebar as a Webpage dashboard that
// shows the Vite dev server. The dev HA is a plain container without the
// Supervisor, so it can't run add-ons or Ingress; on a real HA OS install
// the add-on's own Ingress panel does this instead.
//
// Usage: node dev/ha-sidebar.mjs [url]   (default http://127.0.0.1:5173/)

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const env = Object.fromEntries(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '.env'), 'utf8')
    .split('\n')
    .filter((l) => l.includes('=') && !l.startsWith('#'))
    .map((l) => [l.slice(0, l.indexOf('=')), l.slice(l.indexOf('=') + 1)]),
)
const HA = env.ERGO_HA_URL ?? 'http://localhost:8123'
const URL_PATH = 'ergo-dev' // dashboard paths must contain a hyphen
const TITLE = 'Ergo'
const target = process.argv[2] ?? 'http://127.0.0.1:5173/'

const ws = new WebSocket(`${HA.replace(/^http/, 'ws')}/api/websocket`)
let id = 0
const pending = new Map()
const call = (msg) =>
  new Promise((resolve, reject) => {
    const n = ++id
    pending.set(n, { resolve, reject })
    ws.send(JSON.stringify({ id: n, ...msg }))
  })

ws.onmessage = async (ev) => {
  const msg = JSON.parse(ev.data)
  if (msg.type === 'auth_required') return ws.send(JSON.stringify({ type: 'auth', access_token: env.ERGO_HA_TOKEN }))
  if (msg.type === 'auth_invalid') throw new Error('HA token rejected: run make dev-bootstrap')
  if (msg.type === 'result') {
    const p = pending.get(msg.id)
    pending.delete(msg.id)
    return msg.success ? p.resolve(msg.result) : p.reject(new Error(JSON.stringify(msg.error)))
  }
  if (msg.type !== 'auth_ok') return

  try {
    const dashboards = await call({ type: 'lovelace/dashboards/list' })
    const existing = dashboards.find((d) => d.url_path === URL_PATH)
    if (!existing) {
      await call({
        type: 'lovelace/dashboards/create',
        url_path: URL_PATH,
        title: TITLE,
        icon: 'mdi:sitemap',
        show_in_sidebar: true,
        require_admin: true,
        mode: 'storage',
      })
      console.log('created the ergo sidebar entry')
    } else {
      await call({ type: 'lovelace/dashboards/update', dashboard_id: existing.id, title: TITLE, icon: 'mdi:sitemap' })
      console.log('ergo sidebar entry already exists; updated it')
    }
    // A "Webpage" dashboard is a dashboard driven by the iframe strategy.
    await call({ type: 'lovelace/config/save', url_path: URL_PATH, config: { strategy: { type: 'iframe', url: target } } })

    // Sidebar order is a per-user frontend preference. Put Ergo last and keep
    // everything else as it is (panels HA hides by default stay hidden).
    const { value: saved } = await call({ type: 'frontend/get_user_data', key: 'sidebar' })
    const panels = Object.keys(await call({ type: 'get_panels' }))
    const internal = ['config', 'profile', 'notfound', 'developer-tools']
    const hiddenByDefault = ['light', 'security', 'climate', 'maintenance']
    // In current HA the Overview panel is `home`.
    const defaultOrder = ['home', 'lovelace', 'map', 'logbook', 'history', 'media-browser', 'todo']
    const order = (saved?.panelOrder ?? [
      ...defaultOrder.filter((p) => panels.includes(p)),
      ...panels.filter((p) => !defaultOrder.includes(p) && !internal.includes(p)),
    ])
      .filter((p) => p !== URL_PATH)
      .concat(URL_PATH)
    const hidden = saved?.hiddenPanels ?? hiddenByDefault.filter((p) => panels.includes(p))
    await call({ type: 'frontend/set_user_data', key: 'sidebar', value: { ...saved, panelOrder: order, hiddenPanels: hidden } })
    console.log('Ergo is last in the sidebar')
    console.log(`sidebar "${TITLE}" shows ${target}: ${HA}/${URL_PATH}`)
  } catch (e) {
    console.error(e.message)
    process.exitCode = 1
  }
  ws.close()
}
