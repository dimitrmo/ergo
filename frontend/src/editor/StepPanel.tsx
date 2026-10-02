import { useState } from 'react'
import type { Entity, NodeSchema, RunNode } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { nodeIcon } from '../describe.ts'
import { EntityPicker, FieldBlock, FieldInput, type InsertChip, MoreOptions, SchedulePicker, StateChips, TemplateText } from './fields.tsx'
import { ComposeForm, DownloadForm, FilterForm, IfForm, MapForm, ParseForm, RequestForm } from './dataForms.tsx'
import { ActionForm, MqttTriggerForm } from './haForms.tsx'
import { JsonTree } from './JsonTree.tsx'
import { type ErgoNode, PORT_LABEL, useEditor } from './model.ts'

type Patch = { label?: string; config?: Record<string, unknown> }

/** Readable label for a field name: `entity_id` -> "Entity id". */
function labelOf(key: string): string {
  const words = key.replace(/_/g, ' ')
  return words.charAt(0).toUpperCase() + words.slice(1)
}

/**
 * Data a step can insert: the fields of its input (the previous steps'
 * output schemas) plus the run's time.
 */
function insertChips(upstream: NodeSchema[]): InsertChip[] {
  const chips: InsertChip[] = []
  const seen = new Set<string>()
  const add = (chip: InsertChip) => {
    if (!seen.has(chip.value)) {
      seen.add(chip.value)
      chips.push(chip)
    }
  }
  for (const schema of upstream) {
    for (const [key, prop] of Object.entries(schema.output.properties ?? {})) {
      // Skip bookkeeping fields and nested objects (they have their own paths).
      if (['node', 'id', 'kind', 'scheduled_iso'].includes(key) || prop.type === 'object') continue
      add({ label: labelOf(key), value: `{{ input.${key} }}`, title: prop.description })
      if (key === 'items' && prop.type === 'array') {
        add({ label: 'First item', value: '{{ input.items[0] }}', title: 'The first item; add .title etc. for one field' })
      }
    }
    if (schema.type === 'trigger.state') {
      add({ label: 'Entity name', value: '{{ input.to_state.attributes.friendly_name }}', title: 'Friendly name of the entity' })
    }
    if (schema.type === 'trigger.mqtt') {
      add({ label: 'Message JSON', value: '{{ input.json }}', title: 'The message parsed as JSON; add .field for one value' })
    }
  }
  // Right after a trigger, `input.time` already is the run's time.
  if (!chips.some((c) => c.label === 'Time')) {
    add({ label: 'Time', value: '{{ trigger.time }}', title: 'When the run started, in HA’s time zone' })
  }
  if (upstream.length) add({ label: 'All input as JSON', value: '{{ input | tojson }}', title: 'The whole input as JSON text' })
  return chips
}

const QOS = [
  { v: '0', label: 'Fire and forget' },
  { v: '1', label: 'At least once' },
  { v: '2', label: 'Exactly once' },
]

function Form({ node, onChange, upstream }: { node: ErgoNode; onChange: (p: Patch) => void; upstream: NodeSchema[] }) {
  const { schemas, entities } = useEditor()
  const cfg = node.data.config
  const s = (k: string) => (typeof cfg[k] === 'string' ? (cfg[k] as string) : cfg[k] == null ? '' : String(cfg[k]))
  const set = (k: string, v: unknown) => onChange({ config: { ...cfg, [k]: v } })
  const setMany = (patch: Record<string, unknown>) => onChange({ config: { ...cfg, ...patch } })
  const nickname = (
    <FieldBlock label="Nickname" help="Optional. Later steps can tell triggers apart with {{ trigger.id }}.">
      <input
        className="input mono"
        value={node.data.label ?? ''}
        placeholder="e.g. morning"
        onChange={(e) => onChange({ label: e.target.value.replace(/\s+/g, '_') })}
      />
    </FieldBlock>
  )

  switch (node.data.nodeType) {
    case 'trigger.state': {
      const entity = entities.find((e: Entity) => e.entity_id === s('entity_id'))
      return (
        <>
          <FieldBlock label="What should ergo watch?">
            <EntityPicker value={s('entity_id')} onChange={(v) => set('entity_id', v)} entities={entities} />
          </FieldBlock>
          {s('entity_id') && (
            <FieldBlock label="When it changes to">
              <StateChips value={s('to')} onChange={(v) => set('to', v)} entity={entity} anyLabel="Anything" />
            </FieldBlock>
          )}
          <MoreOptions>
            <FieldBlock label="Only if it was">
              <StateChips value={s('from')} onChange={(v) => set('from', v)} entity={entity} anyLabel="Anything" />
            </FieldBlock>
            {nickname}
          </MoreOptions>
        </>
      )
    }
    case 'trigger.cron':
      return (
        <>
          <SchedulePicker value={s('cron')} onChange={(v) => set('cron', v)} />
          <MoreOptions>{nickname}</MoreOptions>
        </>
      )
    case 'trigger.manual':
      return (
        <>
          <p className="quiet">This workflow starts whenever you press Try it. Handy while you're building.</p>
          <MoreOptions>{nickname}</MoreOptions>
        </>
      )
    case 'mqtt.publish':
      return (
        <>
          <FieldBlock label="Topic" help="Where the message goes, e.g. home/heater/set">
            <TemplateText value={s('topic')} onChange={(v) => set('topic', v)} placeholder="home/heater/set" chips={[]} />
          </FieldBlock>
          <FieldBlock label="Message">
            <TemplateText
              multiline
              value={s('payload')}
              onChange={(v) => set('payload', v)}
              placeholder="on"
              chips={insertChips(upstream)}
            />
          </FieldBlock>
          <MoreOptions>
            <FieldBlock label="Delivery">
              <div className="chips">
                {QOS.map((q) => (
                  <button key={q.v} className={`chip${(s('qos') || '0') === q.v ? ' on' : ''}`} onClick={() => set('qos', q.v)}>
                    {q.label}
                  </button>
                ))}
              </div>
            </FieldBlock>
            <div className="field toggle-row">
              <button className="switch" role="switch" aria-checked={cfg.retain === true} onClick={() => set('retain', cfg.retain !== true)} />
              <span>Keep it as the topic's last message (retain)</span>
            </div>
          </MoreOptions>
        </>
      )
    case 'trigger.mqtt':
      return <MqttTriggerForm cfg={cfg} set={set} nickname={nickname} />
    case 'ha.action':
      return <ActionForm cfg={cfg} set={set} setMany={setMany} entities={entities} chips={insertChips(upstream)} />
    case 'flow.if':
      return <IfForm cfg={cfg} set={set} chips={insertChips(upstream)} />
    case 'http.request':
      return <RequestForm cfg={cfg} set={set} chips={insertChips(upstream)} />
    case 'http.download':
      return <DownloadForm cfg={cfg} set={set} chips={insertChips(upstream).filter((c) => !c.label.startsWith('All input'))} />
    case 'data.parse': {
      const field = schemas.get('data.parse')?.fields.find((f) => f.key === 'format')
      return <ParseForm cfg={cfg} set={set} formats={field?.type === 'select' ? field.options : ['auto']} />
    }
    case 'data.filter':
      return <FilterForm cfg={cfg} set={set} />
    case 'data.map':
      return <MapForm cfg={cfg} set={set} />
    case 'text.compose':
      return <ComposeForm cfg={cfg} set={set} chips={insertChips(upstream)} />
    default: {
      const schema = schemas.get(node.data.nodeType)
      return (
        <>
          {schema?.fields.map((f) => (
            <FieldBlock key={f.key} label={f.label} help={f.help}>
              <FieldInput field={f} value={cfg[f.key]} onChange={(v) => set(f.key, v)} />
            </FieldBlock>
          ))}
        </>
      )
    }
  }
}

export function StepPanel({
  node,
  result,
  upstream,
  onChange,
  onDelete,
  onClose,
}: {
  node: ErgoNode
  result?: RunNode
  /** Schemas of the steps connected into this one (its input). */
  upstream: NodeSchema[]
  onChange: (p: Patch) => void
  onDelete: () => void
  onClose: () => void
}) {
  const { schemas, issues, stale, onAddAfter } = useEditor()
  const schema = schemas.get(node.data.nodeType)
  const problems = (issues.get(node.id) ?? []).filter((i) => i.severity === 'error')

  return (
    <aside className="drawer">
      <header className={`drawer-head kind-${schema?.kind ?? 'action'}`}>
        <span className="step-icon">
          <Icon name={nodeIcon(node.data.nodeType)} size={22} />
        </span>
        <div>
          <div className="step-kicker">{schema?.kind === 'trigger' ? 'When' : 'Then'}</div>
          <h2>{schema?.title ?? node.data.nodeType}</h2>
        </div>
        <button className="btn ghost icon" onClick={onClose} aria-label="Close">
          <Icon name="x" />
        </button>
      </header>
      <div className="drawer-body">
        {problems.length > 0 && (
          <div className="issue warning">To finish this step: {problems.map((p) => p.message.toLowerCase()).join(', ')}.</div>
        )}
        <Form node={node} onChange={onChange} upstream={upstream} />

        {result && <StepInspector result={result} edited={stale.has(node.id)} />}
      </div>
      <footer className="drawer-foot split">
        {(schema?.ports.length ?? 1) > 1 ? (
          <div className="add-per-port">
            {schema!.ports.map((port) => (
              <button key={port} className="btn" onClick={() => onAddAfter(node.id, port)}>
                <Icon name="plus" size={16} /> Add for “{PORT_LABEL[port] ?? port}”
              </button>
            ))}
          </div>
        ) : (
          <button className="btn" onClick={() => onAddAfter(node.id)}>
            <Icon name="plus" size={16} /> Add a step after this
          </button>
        )}
        <button className="btn danger" onClick={onDelete}>
          <Icon name="trash" size={16} /> Remove
        </button>
      </footer>
    </aside>
  )
}

type Tab = 'input' | 'config' | 'output' | 'error' | 'attempts' | 'logs'

const ERROR_KIND: Record<string, string> = {
  timeout: 'Timed out',
  http_status: 'HTTP error',
  network: 'Network error',
  parse: 'Could not parse',
  template: 'Template error',
  ha: 'Home Assistant error',
  mqtt: 'MQTT error',
  config: 'Setup problem',
  other: 'Error',
}

/** What the step got, did and produced in the run shown on the canvas. */
function StepInspector({ result, edited }: { result: RunNode; edited: boolean }) {
  const tabs: { id: Tab; label: string; show: boolean }[] = [
    { id: 'input', label: 'Input', show: true },
    { id: 'config', label: 'Config', show: result.config !== null },
    { id: 'output', label: 'Output', show: !result.error },
    { id: 'error', label: 'Error', show: !!result.error },
    { id: 'attempts', label: `Attempts${result.attempts.length > 1 ? ` (${result.attempts.length})` : ''}`, show: true },
    { id: 'logs', label: 'Logs', show: result.logs.length > 0 },
  ]
  const [tab, setTab] = useState<Tab>(result.error ? 'error' : 'output')
  const current = tabs.find((t) => t.id === tab && t.show) ? tab : result.error ? 'error' : 'output'

  return (
    <section className="inspector">
      <div className="inspector-head">
        <span className="label">{edited ? 'Before your edits' : 'Last run of this step'}</span>
        <span className="faint">
          {result.port && !result.error ? `left by ${result.port} · ` : ''}
          {result.duration_ms} ms
        </span>
      </div>
      {edited && <p className="faint inspector-note">You changed this step after this run. Try it to see the new result.</p>}
      <div className="inspector-tabs" role="tablist">
        {tabs
          .filter((t) => t.show)
          .map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={current === t.id}
              className={`${current === t.id ? 'on' : ''}${t.id === 'error' ? ' err' : ''}`}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
      </div>
      <div className="inspector-body">
        {current === 'input' && <JsonTree value={result.input} />}
        {current === 'config' && <JsonTree value={result.config} />}
        {current === 'output' && <JsonTree value={result.output} />}
        {current === 'error' && result.error && (
          <div className="inspector-error">
            <div className="issue">
              <strong>{ERROR_KIND[result.error.kind] ?? 'Error'}:</strong> {result.error.message}
            </div>
            {result.error.details !== undefined && <JsonTree value={result.error.details} />}
          </div>
        )}
        {current === 'attempts' && (
          <ol className="attempts">
            {result.attempts.map((a, i) => (
              <li key={i}>
                <span className={`dot ${a.error ? 'err' : 'ok'}`} />
                <span>Attempt {i + 1}</span>
                <span className="faint">
                  {new Date(a.started_at).toLocaleTimeString()} · {a.duration_ms} ms
                </span>
                {a.error && <span className="attempt-error">{a.error.message}</span>}
              </li>
            ))}
          </ol>
        )}
        {current === 'logs' && (
          <pre className="logs">{result.logs.join('\n')}</pre>
        )}
      </div>
    </section>
  )
}
