import { useEffect, useMemo, useRef, useState } from 'react'
import { api, clock, type MqttMessage, type MqttMessages } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { PageHeader } from '../components/PageHeader.tsx'
import { TopBar } from '../components/TopBar.tsx'
import './Database.css'
import './Mqtt.css'

/** Matches what the backend keeps, so the page never holds more. */
const KEEP = 1000
const POLL_MS = 1000
const FILTER_KEY = 'ergo.mqtt.filter'

type Direction = 'all' | 'sent' | 'received'

type Draft = { topic: string; payload: string; qos: number; retain: boolean }
const EMPTY_DRAFT: Draft = { topic: '', payload: '', qos: 0, retain: false }

function savedFilter(): string {
  try {
    return localStorage.getItem(FILTER_KEY) ?? '#'
  } catch {
    return '#'
  }
}

function asJson(text: string): unknown | undefined {
  if (!/^[[{]/.test(text.trim())) return undefined
  try {
    return JSON.parse(text)
  } catch {
    return undefined
  }
}

function size(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`
}

export function Mqtt() {
  const [state, setState] = useState<MqttMessages | null>(null)
  const [messages, setMessages] = useState<MqttMessage[]>([])
  const [frozen, setFrozen] = useState<MqttMessage[] | null>(null)
  const [filterText, setFilterText] = useState(savedFilter)
  const [topic, setTopic] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [direction, setDirection] = useState<Direction>('all')
  const [selected, setSelected] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [sending, setSending] = useState(false)
  const seq = useRef(0)

  // Polls for new messages; this also keeps the backend's watch alive.
  useEffect(() => {
    let alive = true
    const load = async () => {
      try {
        const r = await api.mqttMessages(seq.current)
        if (!alive) return
        // A lower seq means ergo restarted; start over.
        const reset = r.seq < seq.current
        seq.current = r.seq
        setState(r)
        if (reset || r.messages.length) {
          setMessages((m) => (reset ? r.messages : [...m, ...r.messages]).slice(-KEEP))
        }
      } catch (e) {
        if (alive) setError(e instanceof Error ? e.message : String(e))
      }
    }
    load()
    const t = setInterval(load, POLL_MS)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setSelected(null)
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const shown = frozen ?? messages
  const pending = frozen ? messages.length - frozen.length : 0

  const topics = useMemo(() => {
    const by = new Map<string, { count: number; last: MqttMessage }>()
    for (const m of shown) {
      const t = by.get(m.topic)
      by.set(m.topic, { count: (t?.count ?? 0) + 1, last: m })
    }
    return [...by.entries()].sort(([a], [b]) => a.localeCompare(b))
  }, [shown])

  const needle = search.trim().toLowerCase()
  const rows = useMemo(
    () =>
      shown
        .filter(
          (m) =>
            (direction === 'all' || m.direction === direction) &&
            (topic === null || m.topic === topic) &&
            (!needle || m.topic.toLowerCase().includes(needle) || m.payload.toLowerCase().includes(needle)),
        )
        .reverse(),
    [shown, direction, topic, needle],
  )

  const message = selected === null ? null : (shown.find((m) => m.seq === selected) ?? null)

  const watch = async (filter: string | null) => {
    setBusy(true)
    try {
      await api.mqttWatch(filter)
      if (filter) {
        try {
          localStorage.setItem(FILTER_KEY, filter)
        } catch {
          // Storage may be blocked; the filter just isn't remembered.
        }
      }
      setState((s) => s && { ...s, filter })
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const clear = async () => {
    try {
      await api.mqttClear()
      setMessages([])
      setFrozen(frozen && [])
      setSelected(null)
      setTopic(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const send = async () => {
    if (!draft) return
    setSending(true)
    try {
      await api.mqttPublish(draft)
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSending(false)
    }
  }

  const status = state?.status
  const watching = state?.filter ?? null

  return (
    <>
      <TopBar section="mqtt" />
      <main className="page db-page">
        <div className="page-inner db-inner">
          <PageHeader
            title="MQTT"
            subtitle={
              !status ? (
                'Connecting…'
              ) : !status.configured ? (
                'No MQTT broker is set up. Install the Mosquitto add-on, or set mqtt_url.'
              ) : (
                <>
                  <span className={`dot ${status.connected ? 'ok' : 'err'}`} />{' '}
                  {status.connected ? 'Connected to' : 'Not connected to'}{' '}
                  <span className="mono">{status.broker}</span>
                  {!status.connected && status.error && <span className="faint"> · {status.error}</span>}
                </>
              )
            }
            actions={
              <>
                <button className={`btn${draft ? ' active' : ''}`} onClick={() => setDraft(draft ? null : EMPTY_DRAFT)} disabled={!status?.connected}>
                  <Icon name="send" size={16} /> Publish
                </button>
                <button className="btn" onClick={() => setFrozen(frozen ? null : messages)}>
                  <Icon name={frozen ? 'play' : 'clock'} size={16} />
                  {frozen ? `Resume${pending ? ` (${pending} new)` : ''}` : 'Pause'}
                </button>
                <button className="btn" onClick={clear} disabled={!messages.length}>
                  <Icon name="trash" size={16} /> Clear
                </button>
              </>
            }
          />
          {error && <div className="issue">{error}</div>}

          {draft && (
            <form
              className="surface mqtt-publish"
              onSubmit={(e) => {
                e.preventDefault()
                send()
              }}
            >
              <div className="mqtt-publish-row">
                <label className="label" htmlFor="mqtt-pub-topic">
                  Topic
                </label>
                <input
                  id="mqtt-pub-topic"
                  className="input mono"
                  value={draft.topic}
                  onChange={(e) => setDraft({ ...draft, topic: e.target.value })}
                  placeholder="home/test"
                  spellCheck={false}
                  autoFocus
                />
              </div>
              <textarea
                className="textarea mono"
                value={draft.payload}
                onChange={(e) => setDraft({ ...draft, payload: e.target.value })}
                placeholder='Message, e.g. on or {"state": "on"}'
                spellCheck={false}
              />
              <div className="mqtt-publish-row">
                <div className="chips">
                  {[0, 1, 2].map((q) => (
                    <button type="button" key={q} className={`chip small${draft.qos === q ? ' on' : ''}`} onClick={() => setDraft({ ...draft, qos: q })}>
                      QoS {q}
                    </button>
                  ))}
                </div>
                <label className="mqtt-retain">
                  <button
                    type="button"
                    className="switch"
                    role="switch"
                    aria-checked={draft.retain}
                    onClick={() => setDraft({ ...draft, retain: !draft.retain })}
                  />
                  Retain
                </label>
                <span className="spacer" />
                <button type="button" className="btn ghost" onClick={() => setDraft(null)}>
                  Close
                </button>
                <button className="btn primary" disabled={sending || !draft.topic.trim() || /[+#]/.test(draft.topic)}>
                  <Icon name="send" size={16} /> {sending ? 'Sending…' : 'Send'}
                </button>
              </div>
              {draft.retain && (
                <p className="help">The broker keeps it as the topic's last message and hands it to anyone who subscribes later.</p>
              )}
            </form>
          )}

          <form
            className="surface mqtt-watch"
            onSubmit={(e) => {
              e.preventDefault()
              watch(filterText.trim() || '#')
            }}
          >
            <label className="label" htmlFor="mqtt-filter">
              Watch topics
            </label>
            <input
              id="mqtt-filter"
              className="input mono"
              value={filterText}
              onChange={(e) => setFilterText(e.target.value)}
              placeholder="#"
              spellCheck={false}
              disabled={!status?.configured}
            />
            {watching && watching === filterText.trim() ? (
              <button type="button" className="btn" disabled={busy} onClick={() => watch(null)}>
                <Icon name="x" size={16} /> Stop
              </button>
            ) : (
              <button type="submit" className="btn primary" disabled={busy || !status?.connected}>
                <Icon name="play" size={16} /> {watching ? 'Switch' : 'Watch'}
              </button>
            )}
            <p className="help">
              {watching ? (
                <>
                  <span className="pill live">Watching</span> <span className="mono">{watching}</span>. Messages the
                  broker delivers show as <b>received</b>, including retained ones.
                </>
              ) : (
                <>
                  Everything ergo publishes shows here as <b>sent</b>. Watch a filter such as{' '}
                  <span className="mono">#</span> or <span className="mono">zigbee2mqtt/+</span> to also see what the
                  broker delivers.
                </>
              )}
            </p>
          </form>

          <div className={`db-body${message ? ' with-row' : ''}`}>
            <nav className="surface db-tables" aria-label="Topics">
              <button className={`db-table${topic === null ? ' on' : ''}`} onClick={() => setTopic(null)}>
                <span className="db-table-name">All topics</span>
                <span className="db-table-count">{shown.length}</span>
              </button>
              {topics.map(([t, { count, last }]) => (
                <button key={t} className={`db-table${t === topic ? ' on' : ''}`} onClick={() => setTopic(t)}>
                  <span className="db-table-name mono mqtt-topic">{t}</span>
                  <span className="db-table-count">{count}</span>
                  <span className="db-table-about mono mqtt-last">{last.binary ? 'binary' : last.payload || '(empty)'}</span>
                </button>
              ))}
            </nav>

            <section className="surface db-data">
              <header className="db-data-head mqtt-head">
                <div className="chips">
                  {(['all', 'sent', 'received'] as const).map((d) => (
                    <button key={d} className={`chip small${direction === d ? ' on' : ''}`} onClick={() => setDirection(d)}>
                      {d === 'all' ? 'All' : d === 'sent' ? 'Sent' : 'Received'}
                    </button>
                  ))}
                </div>
                <input
                  className="input mqtt-search"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder="Search topics and payloads"
                  spellCheck={false}
                />
              </header>
              <div className="grid-wrap">
                {rows.length === 0 ? (
                  <p className="quiet mqtt-empty">
                    {shown.length === 0
                      ? 'No messages yet. Run a workflow that publishes, or watch a topic filter.'
                      : 'No messages match.'}
                  </p>
                ) : (
                  <table className="grid mqtt-grid">
                    <thead>
                      <tr>
                        <th>Time</th>
                        <th />
                        <th>Topic</th>
                        <th>Payload</th>
                        <th>QoS</th>
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((m) => (
                        <tr key={m.seq} className={selected === m.seq ? 'on' : ''} onClick={() => setSelected(m.seq)}>
                          <td className="mono faint">{clock(m.at)}</td>
                          <td>
                            <span className={`mqtt-dir ${m.direction}`}>{m.direction}</span>
                            {m.retain && <span className="mqtt-flag">retained</span>}
                          </td>
                          <td className="mono">{m.topic}</td>
                          <td className="mono mqtt-payload">
                            {m.binary ? <span className="faint">binary · {size(m.bytes)}</span> : m.payload || <span className="faint">(empty)</span>}
                          </td>
                          <td className="mono faint">{m.qos}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
            </section>

            {message && (
              <Detail
                message={message}
                onClose={() => setSelected(null)}
                onResend={() =>
                  setDraft({ topic: message.topic, payload: message.binary ? '' : message.payload, qos: message.qos, retain: false })
                }
              />
            )}
          </div>
        </div>
      </main>
    </>
  )
}

function Detail({ message: m, onClose, onResend }: { message: MqttMessage; onClose: () => void; onResend: () => void }) {
  const json = m.binary ? undefined : asJson(m.payload)
  return (
    <aside className="surface db-row">
      <header className="drawer-head">
        <span className="step-icon plain">
          <Icon name={m.direction === 'sent' ? 'send' : 'download'} size={22} />
        </span>
        <div>
          <div className="step-kicker">
            {m.direction} · {new Date(m.at).toLocaleString()}
          </div>
          <h2 className="mono mqtt-topic">{m.topic}</h2>
        </div>
        <button className="btn ghost icon" onClick={onClose} aria-label="Close">
          <Icon name="x" />
        </button>
      </header>
      <div className="drawer-body">
        <div className="field">
          <div className="label">
            Payload <span className="faint">· {size(m.bytes)}</span>
            {m.truncated && <span className="faint"> · cut to the first 16 KB</span>}
          </div>
          {json !== undefined ? (
            <pre className="json">{JSON.stringify(json, null, 2)}</pre>
          ) : (
            <div className="value-text mono">{m.payload || <span className="faint">(empty)</span>}</div>
          )}
        </div>
        <div className="field">
          <div className="label">Delivery</div>
          <div className="quiet">
            QoS {m.qos}
            {m.retain ? ', retained by the broker' : ', not retained'}
          </div>
        </div>
        <div className="mqtt-detail-actions">
          <button className="btn" onClick={() => navigator.clipboard?.writeText(m.payload)}>
            <Icon name="braces" size={16} /> Copy payload
          </button>
          <button className="btn" onClick={onResend} disabled={m.binary} title="Open it in the publish form to send again or edit">
            <Icon name="send" size={16} /> Send again…
          </button>
        </div>
      </div>
    </aside>
  )
}
