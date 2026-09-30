// Plain-language descriptions of nodes and schedules, used on the canvas
// and in the workflow list: "Test switch turns on", "Weekdays at 07:00".

import type { Entity, Graph } from './api.ts'
import type { IconName } from './components/Icon.tsx'

const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']

export type Schedule =
  | { kind: 'daily' | 'weekdays' | 'weekends'; time: string }
  | { kind: 'weekly'; time: string; day: number }
  | { kind: 'hourly'; minute: number }
  | { kind: 'every'; minutes: number }
  | { kind: 'custom'; cron: string }

const pad = (n: number) => String(n).padStart(2, '0')

/** Recognises the cron shapes the schedule picker writes. */
export function parseCron(cron: string): Schedule {
  const f = cron.trim().split(/\s+/)
  if (f.length !== 5) return { kind: 'custom', cron }
  const [m, h, dom, mon, dow] = f
  const num = (s: string) => (/^\d+$/.test(s) ? Number(s) : null)
  const every = m.match(/^\*\/(\d+)$/)
  if (every && h === '*' && dom === '*' && mon === '*' && dow === '*') return { kind: 'every', minutes: Number(every[1]) }
  if (num(m) !== null && h === '*' && dom === '*' && mon === '*' && dow === '*') return { kind: 'hourly', minute: num(m)! }
  if (num(m) === null || num(h) === null || dom !== '*' || mon !== '*') return { kind: 'custom', cron }
  const time = `${pad(num(h)!)}:${pad(num(m)!)}`
  if (dow === '*') return { kind: 'daily', time }
  if (dow === '1-5') return { kind: 'weekdays', time }
  if (dow === '0,6' || dow === '6,0' || dow === '6-7' || dow === 'sat,sun') return { kind: 'weekends', time }
  if (num(dow) !== null && num(dow)! <= 7) return { kind: 'weekly', time, day: num(dow)! % 7 }
  return { kind: 'custom', cron }
}

export function toCron(s: Schedule): string {
  if (s.kind === 'custom') return s.cron
  if (s.kind === 'every') return `*/${s.minutes} * * * *`
  if (s.kind === 'hourly') return `${s.minute} * * * *`
  const [h, m] = s.time.split(':').map(Number)
  const dow =
    s.kind === 'weekly' ? String(s.day) : s.kind === 'daily' ? '*' : s.kind === 'weekdays' ? '1-5' : '0,6'
  return `${m} ${h} * * ${dow}`
}

export function describeSchedule(cron: string): string {
  const s = parseCron(cron)
  switch (s.kind) {
    case 'daily':
      return `Every day at ${s.time}`
    case 'weekdays':
      return `Weekdays at ${s.time}`
    case 'weekends':
      return `Weekends at ${s.time}`
    case 'weekly':
      return `Every ${DAYS[s.day]} at ${s.time}`
    case 'hourly':
      return `Every hour at :${pad(s.minute)}`
    case 'every':
      return s.minutes === 1 ? 'Every minute' : `Every ${s.minutes} minutes`
    default:
      return cron.trim() ? `On schedule ${cron}` : 'Pick a schedule'
  }
}

export function entityName(id: string, entities: Entity[]): string {
  return entities.find((e) => e.entity_id === id)?.name ?? id
}

const OP_WORDS: Record<string, string> = {
  equals: 'is',
  not_equals: 'is not',
  contains: 'contains',
  not_contains: 'doesn’t contain',
  starts_with: 'starts with',
  ends_with: 'ends with',
  greater_than: '>',
  less_than: '<',
  exists: 'is set',
  not_exists: 'is not set',
}

const str = (v: unknown) => (typeof v === 'string' ? v.trim() : '')

/** The sentence shown on a node. */
export function describeNode(type: string, config: Record<string, unknown>, entities: Entity[]): string {
  switch (type) {
    case 'trigger.state': {
      const id = str(config.entity_id)
      if (!id) return 'Pick something to watch'
      const name = entityName(id, entities)
      const to = str(config.to)
      const from = str(config.from)
      if (to) return `${name} turns ${to}${from ? ` (from ${from})` : ''}`
      return `${name} changes${from ? ` from ${from}` : ''}`
    }
    case 'trigger.cron':
      return describeSchedule(str(config.cron))
    case 'trigger.manual':
      return 'You start it by hand'
    case 'mqtt.publish': {
      const topic = str(config.topic)
      return topic ? `Send a message to ${topic}` : 'Choose where to send it'
    }
    case 'http.request': {
      const url = str(config.url)
      if (!url) return 'Choose where to send it'
      const method = str(config.method) || 'POST'
      return `${method} ${url.includes('{{') ? url : url.replace(/^https?:\/\//, '').replace(/\/$/, '')}`
    }
    case 'http.download': {
      const url = str(config.url)
      if (!url) return 'Choose what to download'
      return `Download ${url.includes('{{') ? url : url.replace(/^https?:\/\//, '').replace(/\/$/, '')}`
    }
    case 'data.parse': {
      const format = str(config.format)
      return format && format !== 'auto' ? `Read it as ${format.toUpperCase()}` : 'Read it as data'
    }
    case 'data.filter': {
      const rules = Array.isArray(config.rules) ? (config.rules as { field?: string; op?: string; value?: string }[]) : []
      if (config.mode === 'expression') return str(config.expression) ? 'Keep items matching an expression' : 'Choose what to keep'
      if (config.mode === 'jsonata') {
        const j = str(config.jsonata)
        return j ? `Keep items where ${j.length > 44 ? `${j.slice(0, 44)}…` : j}` : 'Choose what to keep'
      }
      const r = rules.find((x) => str(x.field))
      if (!r) return 'Choose what to keep'
      const op = OP_WORDS[r.op ?? 'equals'] ?? r.op
      const more = rules.length > 1 ? ` (+${rules.length - 1})` : ''
      return `Keep items where ${r.field} ${op}${['exists', 'not_exists'].includes(r.op ?? '') ? '' : ` ${r.value ?? ''}`}${more}`
    }
    case 'data.map': {
      if (config.mode === 'jsonata') return str(config.expression) ? 'Reshape it with JSONata' : 'Choose how to reshape it'
      const names = (Array.isArray(config.fields) ? (config.fields as { name?: string }[]) : []).map((f) => str(f.name)).filter(Boolean)
      if (!names.length) return 'Choose the fields to keep'
      return `Keep ${names.slice(0, 3).join(', ')}${names.length > 3 ? ` and ${names.length - 3} more` : ''}`
    }
    case 'text.compose': {
      const t = str(config.template)
      if (!t) return 'Choose the message'
      return `Write “${t.length > 40 ? `${t.slice(0, 40)}…` : t}”`
    }
    default:
      return type
  }
}

/** "When Test switch turns on → send a message to ergo/test" for the workflow list. */
export function describeWorkflow(graph: Graph, entities: Entity[]): { when: string; then: string } {
  const triggers = graph.nodes.filter((n) => n.type.startsWith('trigger.'))
  const steps = graph.nodes.filter((n) => !n.type.startsWith('trigger.'))
  const when = triggers.length
    ? triggers.map((n) => describeNode(n.type, n.config, entities)).join(', or ')
    : 'No trigger yet'
  const then = steps.length
    ? steps.map((n) => describeNode(n.type, n.config, entities).toLowerCase()).join(', then ')
    : 'nothing yet'
  return { when, then }
}

/** States worth offering as chips for an entity. */
export function suggestedStates(entity: Entity | undefined): string[] {
  if (!entity) return []
  if (entity.options?.length) return entity.options
  if (['input_boolean', 'switch', 'light', 'binary_sensor', 'fan', 'automation'].includes(entity.domain)) return ['on', 'off']
  if (entity.domain === 'person' || entity.domain === 'device_tracker') return ['home', 'not_home']
  if (entity.domain === 'cover') return ['open', 'closed']
  if (entity.domain === 'lock') return ['locked', 'unlocked']
  return []
}

/** The icon for each node type. */
export function nodeIcon(type: string): IconName {
  switch (type) {
    case 'trigger.state':
      return 'bolt'
    case 'trigger.cron':
      return 'clock'
    case 'trigger.manual':
      return 'hand'
    case 'mqtt.publish':
      return 'send'
    case 'http.download':
      return 'download'
    case 'http.request':
      return 'globe'
    case 'data.parse':
      return 'braces'
    case 'data.filter':
      return 'funnel'
    case 'data.map':
      return 'shuffle'
    case 'text.compose':
      return 'compose'
    default:
      return 'dots'
  }
}
