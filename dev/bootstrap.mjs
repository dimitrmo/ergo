#!/usr/bin/env node
// Prepares the dev Home Assistant for ergo: completes onboarding (user
// dev / devdevdev), creates a long-lived access token and writes ../.env.
// Safe to re-run: an onboarded instance is logged into instead.

import { existsSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HA = process.env.HA_URL ?? 'http://localhost:8123'
const CLIENT_ID = `${HA}/`
const USER = { name: 'ergo dev', username: 'dev', password: 'devdevdev', language: 'en' }
const ENV_FILE = join(dirname(fileURLToPath(import.meta.url)), '..', '.env')

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function waitForHa() {
  process.stdout.write(`waiting for ${HA} `)
  for (let i = 0; i < 120; i++) {
    try {
      const res = await fetch(`${HA}/api/onboarding`)
      if (res.ok) {
        console.log('up')
        return res.json()
      }
    } catch {
      // not listening yet
    }
    process.stdout.write('.')
    await sleep(2000)
  }
  throw new Error('Home Assistant did not come up within 4 minutes')
}

async function post(path, body, token, form = false) {
  const res = await fetch(`${HA}${path}`, {
    method: 'POST',
    headers: {
      'Content-Type': form ? 'application/x-www-form-urlencoded' : 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: form ? new URLSearchParams(body) : JSON.stringify(body),
  })
  const text = await res.text()
  if (!res.ok) throw new Error(`${path}: ${res.status} ${text}`)
  return text ? JSON.parse(text) : {}
}

async function authCode(steps) {
  const userDone = steps.find((s) => s.step === 'user')?.done
  if (!userDone) {
    console.log('onboarding: creating user dev')
    const { auth_code } = await post('/api/onboarding/users', { client_id: CLIENT_ID, ...USER })
    return { code: auth_code, fresh: true }
  }
  console.log('already onboarded: logging in as dev')
  const flow = await post('/auth/login_flow', {
    client_id: CLIENT_ID,
    handler: ['homeassistant', null],
    redirect_uri: CLIENT_ID,
  })
  const done = await post(`/auth/login_flow/${flow.flow_id}`, {
    client_id: CLIENT_ID,
    username: USER.username,
    password: USER.password,
  })
  if (!done.result) throw new Error(`login failed: ${JSON.stringify(done)}`)
  return { code: done.result, fresh: false }
}

async function finishOnboarding(token) {
  for (const [path, body] of [
    ['/api/onboarding/core_config', {}],
    ['/api/onboarding/analytics', {}],
    ['/api/onboarding/integration', { client_id: CLIENT_ID, redirect_uri: CLIENT_ID }],
  ]) {
    await post(path, body, token).catch((e) => console.log(`  (${e.message.split(':')[0]} skipped)`))
  }
}

function longLivedToken(accessToken) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`${HA.replace(/^http/, 'ws')}/api/websocket`)
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data)
      if (msg.type === 'auth_required') ws.send(JSON.stringify({ type: 'auth', access_token: accessToken }))
      else if (msg.type === 'auth_ok')
        ws.send(
          JSON.stringify({
            id: 1,
            type: 'auth/long_lived_access_token',
            client_name: `ergo dev ${new Date().toISOString().slice(0, 19)}`,
            lifespan: 3650,
          }),
        )
      else if (msg.type === 'result') {
        ws.close()
        msg.success ? resolve(msg.result) : reject(new Error(JSON.stringify(msg.error)))
      } else if (msg.type === 'auth_invalid') reject(new Error('auth invalid'))
    }
    ws.onerror = () => reject(new Error('websocket error'))
  })
}

const steps = await waitForHa()
const { code, fresh } = await authCode(steps)
const tokens = await post('/auth/token', { grant_type: 'authorization_code', code, client_id: CLIENT_ID }, null, true)
if (fresh) await finishOnboarding(tokens.access_token)
const token = await longLivedToken(tokens.access_token)

const existed = existsSync(ENV_FILE)
writeFileSync(
  ENV_FILE,
  [
    '# Written by dev/bootstrap.mjs. Local development only.',
    `ERGO_HA_URL=${HA}`,
    `ERGO_HA_TOKEN=${token}`,
    'ERGO_MQTT_URL=mqtt://localhost:1883',
    'ERGO_DATA_DIR=./dev/data',
    'ERGO_LOG=info,ergo=debug',
    '',
  ].join('\n'),
)
console.log(`${existed ? 'updated' : 'wrote'} .env with a new long-lived token`)
console.log(`HA UI: ${HA}  (user dev / devdevdev)`)
