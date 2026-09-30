import type { ReactNode } from 'react'
import { ErgoLogo } from './ErgoLogo.tsx'

// Inside Home Assistant (Ingress panel or a Webpage dashboard) HA already
// shows the name, so the brand is left out.
const embedded = window.self !== window.top

export type Section = 'workflows' | 'data'

const SECTIONS: { id: Section; label: string; href: string }[] = [
  { id: 'workflows', label: 'Workflows', href: '#/' },
  { id: 'data', label: 'Database', href: '#/data' },
]

export function TopBar({ left, section, children }: { left?: ReactNode; section?: Section; children?: ReactNode }) {
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
            {SECTIONS.map((s) => (
              <a key={s.id} href={s.href} className={`section-tab${section === s.id ? ' on' : ''}`} aria-current={section === s.id ? 'page' : undefined}>
                {s.label}
              </a>
            ))}
          </nav>
        </>
      )}
      <div className="spacer" />
      {children}
    </header>
  )
}
