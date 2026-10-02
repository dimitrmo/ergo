import type { ReactNode } from 'react'
import { useMqttEnabled } from '../hooks.ts'
import { ErgoLogo } from './ErgoLogo.tsx'

// Inside Home Assistant (Ingress panel or a Webpage dashboard) HA already
// shows the name, so the brand is left out.
const embedded = window.self !== window.top

export type Section = 'workflows' | 'data' | 'mqtt'

const SECTIONS: { id: Section; label: string; href: string }[] = [
  { id: 'workflows', label: 'Workflows', href: '#/' },
  { id: 'data', label: 'Database', href: '#/data' },
  { id: 'mqtt', label: 'MQTT', href: '#/mqtt' },
]

export function TopBar({ left, section, children }: { left?: ReactNode; section?: Section; children?: ReactNode }) {
  const mqtt = useMqttEnabled()
  return (
    <header className="topbar">
      {left ?? (
        <>
          {!embedded && (
            <a className="brand" href="#/">
              <ErgoLogo title="ergo" />
              <span>ergo</span>
            </a>
          )}
          <nav className="sections" aria-label="Sections">
            {SECTIONS.filter((s) => s.id !== 'mqtt' || mqtt).map((s) => (
              <a key={s.id} href={s.href} className={`section-tab${section === s.id ? ' on' : ''}`} aria-current={section === s.id ? 'page' : undefined}>
                {s.label}
              </a>
            ))}
          </nav>
        </>
      )}
      <div className="spacer" />
      {children}
      <span className="topbar-version faint hide-narrow" title="ergo version">
        v{__ERGO_VERSION__}
      </span>
    </header>
  )
}
