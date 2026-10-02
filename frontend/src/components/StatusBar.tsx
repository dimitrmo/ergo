import type { Ready } from '../api.ts'

export function StatusBar({ ready }: { ready: Ready | null }) {
  if (!ready) {
    return (
      <footer className="statusbar">
        <span className="item">
          <span className="dot err" /> Can't reach ergo. Is it running?
        </span>
      </footer>
    )
  }
  const { ha_websocket: ha, mqtt } = ready.checks
  return (
    <footer className="statusbar">
      <a className="item" href="#/status" title={ha.error ?? undefined}>
        <span className={`dot ${ha.ok ? 'ok' : 'err'}`} />
        {ha.ok ? 'Home Assistant connected' : 'Home Assistant offline'}
      </a>
      {mqtt.enabled && (
        <a className="item" href="#/status" title={mqtt.error ?? undefined}>
          <span className={`dot ${mqtt.connected ? 'ok' : 'err'}`} />
          {mqtt.connected ? 'MQTT connected' : 'MQTT offline'}
        </a>
      )}
    </footer>
  )
}
