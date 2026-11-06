import type { Ready } from '../api.ts'
import { Icon } from './Icon.tsx'

const REPO_URL = 'https://github.com/dimitrmo/ergo'

function GitHubLink() {
  return (
    <a className="item push-right" href={REPO_URL} target="_blank" rel="noopener noreferrer" title="ergo on GitHub">
      <Icon name="github" size={14} />
      GitHub
    </a>
  )
}

export function StatusBar({ ready }: { ready: Ready | null }) {
  if (!ready) {
    return (
      <footer className="statusbar">
        <span className="item">
          <span className="dot err" /> Can't reach ergo. Is it running?
        </span>
        <GitHubLink />
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
      <GitHubLink />
    </footer>
  )
}
