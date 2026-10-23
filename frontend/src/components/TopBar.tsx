import type { ReactNode } from 'react'
import { useMqttEnabled } from '../hooks.ts'
import { ErgoLogo } from './ErgoLogo.tsx'

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
      <a className="brand" href="#/">
        <ErgoLogo title="ergo" />
      </a>
      {left ?? (
        <>
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
