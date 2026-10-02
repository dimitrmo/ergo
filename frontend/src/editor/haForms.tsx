// Forms for Home Assistant actions and MQTT triggers.

import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { api, type Entity, type HaActionInfo, type HaActions } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { actionWords, entityName } from '../describe.ts'
import { EntityPicker, FieldBlock, type InsertChip, MoreOptions, TemplateText } from './fields.tsx'

type Cfg = Record<string, unknown>
type Set = (key: string, value: unknown) => void

const str = (v: unknown) => (typeof v === 'string' ? v : v == null ? '' : String(v))

/** Entity ids in the field: separated by commas, spaces or new lines. */
const idsOf = (v: unknown) =>
  str(v)
    .split(/[\s,]+/)
    .filter(Boolean)

// HA's catalog rarely changes; fetch it once per page load.
let actionsCache: Promise<HaActions> | null = null
function useHaActions(): { actions: HaActions | null; error: string | null } {
  const [actions, setActions] = useState<HaActions | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    actionsCache ??= api.haActions().catch((e) => {
      actionsCache = null
      throw e
    })
    actionsCache.then(setActions).catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }, [])
  return { actions, error }
}

const COMMON = ['light', 'switch', 'input_boolean', 'climate', 'cover', 'fan', 'lock', 'media_player', 'notify', 'scene', 'script', 'input_number', 'input_select']

/** Searchable list of every action HA offers, common domains first. */
function ActionPicker({ value, actions, onPick }: { value: string; actions: HaActions; onPick: (action: string) => void }) {
  const [open, setOpen] = useState(!value)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const listRef = useRef<HTMLDivElement>(null)

  const all = useMemo(() => {
    const rank = (d: string) => (COMMON.includes(d) ? COMMON.indexOf(d) : 99)
    return Object.entries(actions)
      .flatMap(([domain, services]) => Object.keys(services).map((s) => ({ domain, id: `${domain}.${s}` })))
      .sort((a, b) => rank(a.domain) - rank(b.domain) || a.id.localeCompare(b.id))
  }, [actions])
  const matches = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean)
    // "turn on light" finds light.turn_on: every word appears somewhere.
    return all.filter((a) => words.every((w) => a.id.replace(/_/g, ' ').includes(w) || a.id.includes(w))).slice(0, 80)
  }, [all, query])

  useEffect(() => {
    listRef.current?.querySelector('.active')?.scrollIntoView({ block: 'nearest' })
  }, [active])

  const pick = (id: string) => {
    onPick(id)
    setOpen(false)
    setQuery('')
  }

  if (!open && value) {
    return (
      <button className="entity-card" onClick={() => setOpen(true)}>
        <span className="entity-name">{actionWords(value)}</span>
        <span className="entity-state">{value.split('.')[0]}</span>
        <span className="entity-id mono">{value}</span>
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
        placeholder="Search: turn on light, notify, set temperature…"
        spellCheck={false}
        onChange={(e) => {
          setQuery(e.target.value)
          setActive(0)
        }}
        onKeyDown={(e) => {
          if (e.key === 'ArrowDown') setActive((a) => Math.min(a + 1, matches.length - 1))
          else if (e.key === 'ArrowUp') setActive((a) => Math.max(a - 1, 0))
          else if (e.key === 'Enter' && matches[active]) pick(matches[active].id)
          else if (e.key === 'Escape' && value) setOpen(false)
        }}
      />
      <div className="picker-list" ref={listRef}>
        {matches.map((a, i) => (
          <button
            key={a.id}
            className={`picker-item${i === active ? ' active' : ''}${a.id === value ? ' chosen' : ''}`}
            onMouseEnter={() => setActive(i)}
            onClick={() => pick(a.id)}
          >
            <span className="picker-name">{actionWords(a.id)}</span>
            <span className="picker-state">{a.domain}</span>
            <span className="picker-id mono">{a.id}</span>
          </button>
        ))}
        {!matches.length && <div className="picker-empty">No action matches that.</div>}
      </div>
    </div>
  )
}

/** Adds `key: example` to the data JSON, unless it has templates (then appends). */
function withField(data: string, key: string, example: unknown): string {
  const value = example === undefined ? '' : example
  if (!data.trim()) return JSON.stringify({ [key]: value }, null, 2)
  if (!data.includes('{{') && !data.includes('{%')) {
    try {
      const parsed = JSON.parse(data)
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        return JSON.stringify({ ...parsed, [key]: parsed[key] ?? value }, null, 2)
      }
    } catch {
      // Not JSON yet; fall through and leave the text to the user.
    }
  }
  return data
}

export function ActionForm({
  cfg,
  set,
  setMany,
  entities,
  chips,
}: {
  cfg: Cfg
  set: Set
  /** Sets several keys at once. */
  setMany: (patch: Cfg) => void
  entities: Entity[]
  chips: InsertChip[]
}) {
  const { actions, error } = useHaActions()
  const [adding, setAdding] = useState(false)
  const action = str(cfg.action)
  const [domain, service] = action.split('.')
  const info: HaActionInfo | undefined = actions?.[domain]?.[service]
  const ids = idsOf(cfg.entity_id)
  const templated = str(cfg.entity_id).includes('{{')
  const targetDomains = info?.target?.entity?.flatMap((t) => t.domain ?? []) ?? []
  const pickable = targetDomains.length ? entities.filter((e) => targetDomains.includes(e.domain)) : entities
  // Picked for an earlier action, say: HA would refuse them.
  const misfits = targetDomains.length ? ids.filter((id) => !targetDomains.includes(id.split('.')[0])) : []
  const fields = Object.entries(info?.fields ?? {})
  const data = str(cfg.data)

  const pickAction = (id: string) => {
    const [d, s] = id.split('.')
    const next = actions?.[d]?.[s]
    // Actions that must answer get "wait for its answer" on.
    setMany(next?.response?.optional === false ? { action: id, response: true } : { action: id })
  }

  return (
    <>
      <FieldBlock label="What should Home Assistant do?">
        {actions ? (
          <ActionPicker value={action} actions={actions} onPick={pickAction} />
        ) : error ? (
          <>
            <div className="issue warning">Couldn't load Home Assistant's actions: {error}</div>
            <input className="input mono" value={action} placeholder="light.turn_on" spellCheck={false} onChange={(e) => set('action', e.target.value)} />
          </>
        ) : (
          <p className="quiet">Loading Home Assistant's actions…</p>
        )}
      </FieldBlock>

      {action && (info?.target || ids.length > 0) && !templated && (
        <FieldBlock label={ids.length > 1 ? 'On these' : 'On'}>
          {ids.length > 0 && (
            <div className="target-chips">
              {ids.map((id) => (
                <span key={id} className="chip target-chip">
                  {entityName(id, entities)}
                  <button aria-label={`Remove ${id}`} onClick={() => set('entity_id', ids.filter((x) => x !== id).join(', '))}>
                    <Icon name="x" size={13} />
                  </button>
                </span>
              ))}
            </div>
          )}
          {misfits.length > 0 && (
            <div className="issue warning">
              {action} can't act on {misfits.map((id) => entityName(id, entities)).join(', ')}; it works on{' '}
              {targetDomains.join(', ')} entities.
            </div>
          )}
          {adding || ids.length === 0 ? (
            <EntityPicker
              value=""
              entities={pickable}
              onChange={(id) => {
                if (!ids.includes(id)) set('entity_id', [...ids, id].join(', '))
                setAdding(false)
              }}
            />
          ) : (
            <button className="btn ghost small add-row" onClick={() => setAdding(true)}>
              <Icon name="plus" size={14} /> Add another
            </button>
          )}
        </FieldBlock>
      )}

      {action && (
        <FieldBlock
          label="Options"
          help={
            <>
              A JSON object. Use templates for values from earlier steps, e.g. <code>{'{"brightness_pct": {{ input.level }}}'}</code>; wrap
              text in quotes or use <code>{'{{ input.text | tojson }}'}</code>.
            </>
          }
        >
          <TemplateText multiline lang="json" value={data} onChange={(v) => set('data', v)} placeholder="{}" chips={chips} />
          {fields.length > 0 && (
            <div className="insert-row action-fields">
              <span className="faint">Add</span>
              {fields.map(([key, f]) => (
                <button
                  key={key}
                  className={`chip small${f.required ? ' required' : ''}`}
                  title={f.example !== undefined ? `e.g. ${JSON.stringify(f.example)}` : undefined}
                  onClick={() => set('data', withField(data, key, f.example))}
                >
                  + {key}
                  {f.required ? ' *' : ''}
                </button>
              ))}
            </div>
          )}
        </FieldBlock>
      )}

      {info?.response?.optional === false ? (
        <p className="help">This action answers with data; its answer is this step's output as <code>response</code>.</p>
      ) : null}

      <MoreOptions>
        <FieldBlock label="Entities as text" help="Entity ids separated by commas, or a template like {{ input.entity_id }}.">
          <input className="input mono" value={str(cfg.entity_id)} placeholder="light.kitchen, light.hall" spellCheck={false} onChange={(e) => set('entity_id', e.target.value)} />
        </FieldBlock>
        {info?.response?.optional !== false && (
          <div className="field toggle-row">
            <button className="switch" role="switch" aria-checked={cfg.response === true} onClick={() => set('response', cfg.response !== true)} />
            <span>Wait for its answer (for actions that return data)</span>
          </div>
        )}
      </MoreOptions>
    </>
  )
}

export function MqttTriggerForm({ cfg, set, nickname }: { cfg: Cfg; set: Set; nickname: ReactNode }) {
  return (
    <>
      <FieldBlock
        label="Topic"
        help={
          <>
            Where the messages arrive. <code>+</code> matches one level and <code>#</code> everything below, as in{' '}
            <code>zigbee2mqtt/+/action</code>. The MQTT tab shows what's being sent.
          </>
        }
      >
        <input className="input mono" value={str(cfg.topic)} placeholder="zigbee2mqtt/hall_button/action" spellCheck={false} onChange={(e) => set('topic', e.target.value)} />
      </FieldBlock>
      <FieldBlock label="Only when the message is" help="Leave empty for any message. Exact match, e.g. single.">
        <input className="input mono" value={str(cfg.payload)} placeholder="any" spellCheck={false} onChange={(e) => set('payload', e.target.value)} />
      </FieldBlock>
      <p className="help">
        Retained messages (the last one a topic kept) don't start runs; only new messages do. JSON messages are also there as{' '}
        <code>{'{{ input.json }}'}</code>.
      </p>
      <MoreOptions>{nickname}</MoreOptions>
    </>
  )
}
