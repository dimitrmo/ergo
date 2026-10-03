// Form for the Web push step, and turning notifications on in this browser.

import { useCallback, useEffect, useState } from 'react'
import { api, type PushInfo } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { currentSubscription, guessBrowserName, pushUnavailable, turnOffPush, turnOnPush } from '../push.ts'
import { FieldBlock, type InsertChip, MoreOptions, TemplateText } from './fields.tsx'

type Cfg = Record<string, unknown>
type Set = (key: string, value: unknown) => void

const str = (v: unknown) => (typeof v === 'string' ? v : v == null ? '' : String(v))

const URGENCY_LABEL: Record<string, string> = {
  'very-low': 'Very low',
  low: 'Low',
  normal: 'Normal',
  high: 'High',
}

const URGENCY_HELP: Record<string, string> = {
  'very-low': 'Delivered when the device is on power and Wi-Fi, e.g. a daily summary.',
  low: 'Delivered when the device is on power or Wi-Fi.',
  normal: 'Delivered soon, unless the device is saving battery.',
  high: 'Delivered right away and stays on screen until dismissed. For alarms and doors left open.',
}

/** This browser: turn notifications on or off, and send a test. */
function ThisBrowser({ info, reload }: { info: PushInfo; reload: () => void }) {
  const unavailable = pushUnavailable()
  const [endpoint, setEndpoint] = useState<string | null>(null)
  const [name, setName] = useState(guessBrowserName)
  const [busy, setBusy] = useState(false)
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null)

  useEffect(() => {
    currentSubscription()
      .then((s) => setEndpoint(s?.endpoint ?? null))
      .catch(() => setEndpoint(null))
  }, [info])

  const mine = info.subscriptions.find((s) => s.endpoint === endpoint)
  const act = async (fn: () => Promise<string>) => {
    setBusy(true)
    setNote(null)
    try {
      setNote({ ok: true, text: await fn() })
    } catch (e) {
      setNote({ ok: false, text: e instanceof Error ? e.message : String(e) })
    } finally {
      setBusy(false)
      reload()
    }
  }

  if (unavailable) return <div className="issue warning">{unavailable}</div>
  return (
    <div className="push-here">
      {mine ? (
        <>
          <p>
            <Icon name="check" size={15} /> This browser gets notifications as <strong>{mine.name}</strong>.
          </p>
          <div className="push-actions">
            <button
              className="btn small"
              disabled={busy}
              onClick={() => act(async () => (await api.pushTest(mine.id), 'Test sent. It should appear in a moment.'))}
            >
              <Icon name="bell" size={14} /> Send a test
            </button>
            <button className="btn ghost small" disabled={busy} onClick={() => act(async () => (await turnOffPush(mine.id), 'Turned off here.'))}>
              Turn off here
            </button>
          </div>
        </>
      ) : (
        <>
          <p className="quiet">Get these notifications on this computer or phone too.</p>
          <div className="push-actions">
            <input className="input" value={name} aria-label="Name for this browser" onChange={(e) => setName(e.target.value)} />
            <button
              className="btn primary small"
              disabled={busy || !name.trim()}
              onClick={() => act(async () => (await turnOnPush(name.trim(), info.public_key), 'Notifications are on for this browser.'))}
            >
              <Icon name="bell" size={14} /> Turn on here
            </button>
          </div>
        </>
      )}
      {note && <p className={note.ok ? 'help' : 'issue warning'}>{note.text}</p>}
    </div>
  )
}

/** Web push: who gets it, the title and message, and how urgent it is. */
export function PushForm({ cfg, set, chips }: { cfg: Cfg; set: Set; chips: InsertChip[] }) {
  const [info, setInfo] = useState<PushInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const load = useCallback(() => {
    api
      .pushInfo()
      .then(setInfo)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }, [])
  useEffect(load, [load])

  const to = str(cfg.to) || 'all'
  const urgency = str(cfg.urgency) || 'normal'
  const gone = to !== 'all' && info && !info.subscriptions.some((s) => s.id === to)
  return (
    <>
      <FieldBlock label="Send to">
        {info ? (
          <>
            <div className="chips">
              <button className={`chip${to === 'all' ? ' on' : ''}`} onClick={() => set('to', 'all')}>
                All browsers{info.subscriptions.length ? ` (${info.subscriptions.length})` : ''}
              </button>
              {info.subscriptions.map((s) => (
                <button key={s.id} className={`chip${to === s.id ? ' on' : ''}`} onClick={() => set('to', s.id)}>
                  {s.name}
                </button>
              ))}
            </div>
            {gone && <div className="issue warning">That browser turned notifications off. Pick another, or All browsers.</div>}
            {!info.subscriptions.length && <p className="help">No browser gets notifications yet. Turn them on below.</p>}
            <ThisBrowser info={info} reload={load} />
          </>
        ) : error ? (
          <div className="issue warning">Couldn't load the browsers: {error}</div>
        ) : (
          <p className="quiet">Loading…</p>
        )}
      </FieldBlock>
      <FieldBlock label="Message" help="Shows under the title Ergo.">
        <TemplateText multiline value={str(cfg.message)} onChange={(v) => set('message', v)} placeholder="The garage door is still open" chips={chips} />
      </FieldBlock>
      <FieldBlock label="Urgency" help={URGENCY_HELP[urgency]}>
        <div className="chips">
          {(info?.urgencies ?? Object.keys(URGENCY_LABEL)).map((u) => (
            <button key={u} className={`chip${urgency === u ? ' on' : ''}`} onClick={() => set('urgency', u)}>
              {URGENCY_LABEL[u] ?? u}
            </button>
          ))}
        </div>
      </FieldBlock>
      <MoreOptions>
        <FieldBlock label="Opens" help="Where a click on the notification goes. Empty opens ergo.">
          <TemplateText value={str(cfg.url)} onChange={(v) => set('url', v)} placeholder="https://…" chips={chips} />
        </FieldBlock>
      </MoreOptions>
    </>
  )
}
