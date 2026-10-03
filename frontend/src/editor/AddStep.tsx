import { useEffect } from 'react'
import type { NodeSchema } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { nodeIcon } from '../describe.ts'

const FRIENDLY: Record<string, string> = {
  'trigger.state': 'When a light, sensor, switch or person changes.',
  'trigger.cron': 'At set times: every morning, on weekdays, every few minutes.',
  'trigger.manual': 'Only when you press Try it. Great for testing.',
  'trigger.mqtt': 'When a message arrives on an MQTT topic, like a button press.',
  'ha.action': 'Turn on a light, set a thermostat, lock a door: any Home Assistant action.',
  'ha.notify': 'Send a notification to your phone, with a title and a message.',
  'push.send': 'Pop up a notification in your browser, on any computer that turned them on.',
  'flow.if': 'Go one way when something is true, another way when it isn’t. Check values, the time, the day or the sun.',
  'flow.delay': 'Wait a few seconds, minutes or hours before the next step.',
  'flow.wait': 'Wait until something happens, like a door closing, or give up after a while.',
  'mqtt.publish': 'Send a message to your MQTT broker.',
  'http.request': 'Call a web API or webhook: any method, headers and body.',
  'http.download': 'Fetch a feed, file or API response from the web.',
  'data.parse': 'Turn XML or JSON into data the next steps can use.',
  'data.filter': 'Keep only the items you care about.',
  'data.map': 'Pick and rename the fields you need.',
  'text.compose': 'Write a message using data from earlier steps.',
}

/** The big "what next?" chooser: triggers for a new start, steps otherwise. */
export function AddStep({
  mode,
  schemas,
  onPick,
  onClose,
}: {
  mode: 'trigger' | 'step'
  schemas: Map<string, NodeSchema>
  onPick: (type: string) => void
  onClose?: () => void
}) {
  const order = Object.keys(FRIENDLY)
  const items = [...schemas.values()]
    .filter((s) => (mode === 'trigger') === (s.kind === 'trigger'))
    .sort((a, b) => (order.indexOf(a.type) + 1 || 99) - (order.indexOf(b.type) + 1 || 99))

  useEffect(() => {
    if (!onClose) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const body = (
    <div className="chooser" onClick={(e) => e.stopPropagation()}>
      <div className="chooser-head">
        <h2>{mode === 'trigger' ? 'How should this workflow start?' : 'What should happen next?'}</h2>
        {onClose && (
          <button className="btn ghost icon" onClick={onClose} aria-label="Close">
            <Icon name="x" />
          </button>
        )}
      </div>
      <div className={`chooser-grid${mode === 'trigger' ? ' two-up' : ''}`}>
        {items.map((s) => (
          <button key={s.type} className={`choice kind-${s.kind}`} onClick={() => onPick(s.type)}>
            <span className="choice-icon">
              <Icon name={nodeIcon(s.type)} size={26} />
            </span>
            <span className="choice-title">{s.title}</span>
            <span className="choice-desc">{FRIENDLY[s.type] ?? s.description}</span>
          </button>
        ))}
      </div>
    </div>
  )

  // As a modal it gets a backdrop; on an empty canvas it sits inline.
  return onClose ? (
    <div className="backdrop" onClick={onClose}>
      {body}
    </div>
  ) : (
    <div className="chooser-inline">{body}</div>
  )
}
