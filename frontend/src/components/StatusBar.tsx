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
      <a className="item" href="#/status" title={mqtt.error ?? undefined}>
        <span className={`dot ${!mqtt.configured ? '' : mqtt.connected ? 'ok' : 'err'}`} />
        {!mqtt.configured ? 'No MQTT broker' : mqtt.connected ? 'MQTT connected' : 'MQTT offline'}
      </a>
      <span className="spacer" />
      <span className="item faint hide-narrow">ergo {ready.version}</span>
    </footer>
  )
}
