import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { api, type Entity, type Field } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { parseCron, type Schedule, suggestedStates, toCron } from '../describe.ts'
import { useDebounced } from '../hooks.ts'
import { HighlightedTextarea } from './highlight.tsx'

export function FieldBlock({ label, help, children }: { label: string; help?: ReactNode; children: ReactNode }) {
  return (
    <div className="field">
      <div className="label">{label}</div>
      {children}
      {help && <div className="help">{help}</div>}
    </div>
  )
}

export function MoreOptions({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false)
  return (
    <div className={`more${open ? ' open' : ''}`}>
      <button className="more-toggle" onClick={() => setOpen(!open)} aria-expanded={open}>
        <Icon name="back" size={14} className="more-caret" />
        More options
      </button>
      {open && <div className="more-body">{children}</div>}
    </div>
  )
}

/** Generic input for node types without a hand-made form. */
export function FieldInput({ field, value, onChange }: { field: Field; value: unknown; onChange: (v: unknown) => void }) {
  const str = value === undefined || value === null ? '' : String(value)
  if (field.type === 'select')
    return (
      <div className="chips">
        {field.options.map((o) => (
          <button key={o} className={`chip${str === o ? ' on' : ''}`} onClick={() => onChange(o)}>
            {o}
          </button>
        ))}
      </div>
    )
  if (field.type === 'bool')
    return (
      <button className="switch" role="switch" aria-checked={value === true} onClick={() => onChange(value !== true)} />
    )
  return (
    <input
      className={`input${field.type === 'template' ? ' mono' : ''}`}
      value={str}
      placeholder={field.placeholder}
      spellCheck={false}
      onChange={(e) => onChange(e.target.value)}
    />
  )
}

const DOMAIN_ORDER = ['input_boolean', 'switch', 'light', 'binary_sensor', 'sensor', 'person', 'input_number', 'input_select']

/** Searchable list of live HA entities, names first. */
export function EntityPicker({
  value,
  onChange,
  entities,
}: {
  value: string
  onChange: (v: string) => void
  entities: Entity[]
}) {
  const [open, setOpen] = useState(!value)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const listRef = useRef<HTMLDivElement>(null)
  const current = entities.find((e) => e.entity_id === value)

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase()
    const rank = (e: Entity) => {
      const i = DOMAIN_ORDER.indexOf(e.domain)
      return i < 0 ? 99 : i
    }
    return entities
      .filter((e) => !q || e.entity_id.includes(q) || e.name.toLowerCase().includes(q))
      .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))
      .slice(0, 60)
  }, [entities, query])

  useEffect(() => {
    listRef.current?.querySelector('.active')?.scrollIntoView({ block: 'nearest' })
  }, [active])

  const pick = (id: string) => {
    onChange(id)
    setOpen(false)
    setQuery('')
  }

  if (!open && current) {
    return (
      <button className="entity-card" onClick={() => setOpen(true)}>
        <span className="entity-name">{current.name}</span>
        <span className="entity-state">
          {current.state}
          {current.unit ? ` ${current.unit}` : ''}
        </span>
        <span className="entity-id mono">{current.entity_id}</span>
        <span className="entity-change">Change</span>
      </button>
    )
  }

  return (
    <div className="picker">
      <input
        className="input"
        autoFocus
        value={query}
        placeholder="Search: kitchen light, temperature, door…"
        spellCheck={false}
        onChange={(e) => {
          setQuery(e.target.value)
          setActive(0)
        }}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown') setActive((a) => Math.min(a + 1, matches.length - 1))
          else if (e.key === 'ArrowUp') setActive((a) => Math.max(a - 1, 0))
          else if (e.key === 'Enter' && matches[active]) pick(matches[active].entity_id)
          else if (e.key === 'Escape' && value) setOpen(false)
        }}
      />
      <div className="picker-list" ref={listRef}>
        {matches.map((e, i) => (
          <button
            key={e.entity_id}
            className={`picker-item${i === active ? ' active' : ''}${e.entity_id === value ? ' chosen' : ''}`}
            onMouseEnter={() => setActive(i)}
            onClick={() => pick(e.entity_id)}
          >
            <span className="picker-name">{e.name}</span>
            <span className="picker-state">
              {e.state}
              {e.unit ? ` ${e.unit}` : ''}
            </span>
            <span className="picker-id mono">{e.entity_id}</span>
          </button>
        ))}
        {!matches.length && (
          <div className="picker-empty">
            {entities.length ? 'Nothing matches that.' : 'No entities yet. Is Home Assistant connected?'}
          </div>
        )}
      </div>
    </div>
  )
}

const UNITS: [string, string][] = [
  ['seconds', 'Seconds'],
  ['minutes', 'Minutes'],
  ['hours', 'Hours'],
]

/** A number and a unit (seconds, minutes, hours), as Delay and Wait until use them. */
export function DurationField({
  amount,
  unit,
  onAmount,
  onUnit,
}: {
  amount: unknown
  unit: unknown
  onAmount: (v: number | string) => void
  onUnit: (v: string) => void
}) {
  const u = typeof unit === 'string' && unit ? unit : 'minutes'
  return (
    <div className="duration">
      <input
        className="input duration-amount"
        type="number"
        min={0}
        step="any"
        value={amount == null ? '' : String(amount)}
        onChange={(e) => onAmount(e.target.value === '' ? '' : Number(e.target.value))}
      />
      <div className="chips">
        {UNITS.map(([v, l]) => (
          <button key={v} className={`chip${u === v ? ' on' : ''}`} onClick={() => onUnit(v)}>
            {l}
          </button>
        ))}
      </div>
    </div>
  )
}

/** "Changes to" as chips: common states for the entity, Anything, or your own. */
export function StateChips({
  value,
  onChange,
  entity,
  anyLabel,
}: {
  value: string
  onChange: (v: string) => void
  entity?: Entity
  /** The "any state" chip; left out when a state must be picked. */
  anyLabel?: string
}) {
  const suggested = suggestedStates(entity)
  const custom = !!value && !suggested.includes(value)
  const [typing, setTyping] = useState(custom)
  return (
    <>
      <div className="chips">
        {anyLabel && (
          <button className={`chip${!value && !typing ? ' on' : ''}`} onClick={() => (setTyping(false), onChange(''))}>
            {anyLabel}
          </button>
        )}
        {suggested.map((s) => (
          <button key={s} className={`chip${value === s && !typing ? ' on' : ''}`} onClick={() => (setTyping(false), onChange(s))}>
            {s}
          </button>
        ))}
        <button className={`chip${typing ? ' on' : ''}`} onClick={() => setTyping(true)}>
          Something else…
        </button>
      </div>
      {typing && (
        <input
          className="input mono chip-input"
          autoFocus
          value={value}
          placeholder="exact state, e.g. heat"
          onChange={(e) => onChange(e.target.value)}
        />
      )}
    </>
  )
}

const PRESETS: { kind: Schedule['kind']; label: string }[] = [
  { kind: 'daily', label: 'Every day' },
  { kind: 'weekdays', label: 'Weekdays' },
  { kind: 'weekends', label: 'Weekends' },
  { kind: 'weekly', label: 'Once a week' },
  { kind: 'hourly', label: 'Every hour' },
  { kind: 'every', label: 'Every few minutes' },
  { kind: 'custom', label: 'Custom' },
]
const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']
const whenFmt = new Intl.DateTimeFormat([], { weekday: 'long', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })

function switchKind(from: Schedule, kind: Schedule['kind']): Schedule {
  const time = 'time' in from ? from.time : '07:00'
  switch (kind) {
    case 'daily':
    case 'weekdays':
    case 'weekends':
      return { kind, time }
    case 'weekly':
      return { kind, time, day: 1 }
    case 'hourly':
      return { kind, minute: 0 }
    case 'every':
      return { kind, minutes: 5 }
    default:
      return { kind: 'custom', cron: toCron(from) }
  }
}

/** Presets and a time picker instead of raw cron; cron stays under Custom. */
export function SchedulePicker({ value, onChange }: { value: string; onChange: (cron: string) => void }) {
  const schedule = parseCron(value || '0 7 * * *')
  const [forceCustom, setForceCustom] = useState(false)
  const kind = forceCustom ? 'custom' : schedule.kind
  const set = (s: Schedule) => onChange(toCron(s))

  const [preview, setPreview] = useState<{ next: string[]; tz: string } | { error: string } | null>(null)
  useDebounced(
    () => {
      if (!value.trim()) return setPreview(null)
      api
        .cronPreview(value)
        .then((r) => setPreview({ next: r.next.slice(0, 3), tz: r.time_zone }))
        .catch((e) => setPreview({ error: e.message }))
    },
    [value],
    250,
  )

  return (
    <>
      <FieldBlock label="How often?">
        <div className="chips">
          {PRESETS.map((p) => (
            <button
              key={p.kind}
              className={`chip${kind === p.kind ? ' on' : ''}`}
              onClick={() => {
                setForceCustom(p.kind === 'custom')
                if (p.kind !== 'custom') set(switchKind(schedule, p.kind))
              }}
            >
              {p.label}
            </button>
          ))}
        </div>
      </FieldBlock>

      {kind !== 'custom' && 'time' in schedule && (
        <div className="row-fields">
          {schedule.kind === 'weekly' && (
            <FieldBlock label="On">
              <select className="select" value={schedule.day} onChange={(e) => set({ ...schedule, day: Number(e.target.value) })}>
                {DAYS.map((d, i) => (
                  <option key={d} value={i}>
                    {d}
                  </option>
                ))}
              </select>
            </FieldBlock>
          )}
          <FieldBlock label="At">
            <input
              className="input time-input"
              type="time"
              value={schedule.time}
              onChange={(e) => e.target.value && set({ ...schedule, time: e.target.value })}
            />
          </FieldBlock>
        </div>
      )}

      {kind === 'hourly' && schedule.kind === 'hourly' && (
        <FieldBlock label="Minutes past the hour">
          <input
            className="input narrow"
            type="number"
            min={0}
            max={59}
            value={schedule.minute}
            onChange={(e) => set({ kind: 'hourly', minute: Math.min(59, Math.max(0, Number(e.target.value))) })}
          />
        </FieldBlock>
      )}

      {kind === 'every' && schedule.kind === 'every' && (
        <FieldBlock label="Every">
          <div className="chips">
            {[1, 5, 10, 15, 30].map((m) => (
              <button key={m} className={`chip${schedule.minutes === m ? ' on' : ''}`} onClick={() => set({ kind: 'every', minutes: m })}>
                {m} min
              </button>
            ))}
          </div>
        </FieldBlock>
      )}

      {kind === 'custom' && (
        <FieldBlock label="Cron expression" help="minute · hour · day of month · month · day of week">
          <input className="input mono" value={value} spellCheck={false} onChange={(e) => onChange(e.target.value)} />
        </FieldBlock>
      )}

      {preview && 'error' in preview && <div className="issue">That schedule doesn't parse: {preview.error}</div>}
      {preview && 'next' in preview && (
        <div className="next-runs">
          <div className="label">Next runs</div>
          {preview.next.map((t) => (
            <div key={t} className="next-run">
              <Icon name="clock" size={15} /> {whenFmt.format(new Date(t))}
            </div>
          ))}
          <div className="help">Times are in Home Assistant's time zone ({preview.tz}).</div>
        </div>
      )}
    </>
  )
}

export interface InsertChip {
  label: string
  value: string
  title?: string
}

/** A text box with one-click chips that insert data from earlier steps. */
export function TemplateText({
  value,
  onChange,
  placeholder,
  chips,
  multiline,
  lang,
}: {
  value: string
  onChange: (v: string) => void
  placeholder?: string
  chips: InsertChip[]
  multiline?: boolean
  /** Also colour JSON outside the template tags. */
  lang?: 'text' | 'json'
}) {
  const ref = useRef<HTMLTextAreaElement & HTMLInputElement>(null)
  const insert = (text: string) => {
    const el = ref.current
    const start = el?.selectionStart ?? value.length
    const end = el?.selectionEnd ?? value.length
    const next = value.slice(0, start) + text + value.slice(end)
    onChange(next)
    requestAnimationFrame(() => {
      el?.focus()
      el?.setSelectionRange(start + text.length, start + text.length)
    })
  }
  const props = {
    ref,
    value,
    placeholder,
    spellCheck: false,
    onChange: (e: { target: { value: string } }) => onChange(e.target.value),
  }
  return (
    <>
      {multiline ? <HighlightedTextarea className="textarea mono message" lang={lang} {...props} /> : <input className="input mono" {...props} />}
      {chips.length > 0 && (
        <div className="insert-row">
          <span className="faint">Insert</span>
          {chips.map((c) => (
            <button key={c.value} className="chip small" onClick={() => insert(c.value)} title={c.title ? `${c.title}: ${c.value}` : c.value}>
              + {c.label}
            </button>
          ))}
        </div>
      )}
    </>
  )
}
