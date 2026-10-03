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

/** An If or Filter rule: a value test, or (If only) the time, day or sun. */
export type Rule = {
  kind?: 'value' | 'time' | 'weekday' | 'sun'
  field?: string
  op?: string
  value?: string
  after?: string
  before?: string
  days?: number[]
  sun?: string
}

const SUN_WORDS: Record<string, string> = {
  after_sunset: 'after sunset',
  before_sunrise: 'before sunrise',
  down: 'the sun is down',
  up: 'the sun is up',
}

/** [1..5] -> "on weekdays", [0, 6] -> "at weekends", else "on Mon, Wed". */
export function describeDays(days: number[]): string {
  const set = [...new Set(days)].sort()
  if (set.join() === '1,2,3,4,5') return 'on weekdays'
  if (set.join() === '0,6') return 'at weekends'
  if (set.length === 7) return 'on any day'
  return `on ${set.map((d) => DAYS[d]?.slice(0, 3)).join(', ')}`
}

function describeRule(r: Rule): string {
  switch (r.kind) {
    case 'time': {
      const after = str(r.after)
      const before = str(r.before)
      if (after && before) return `between ${after} and ${before}`
      return after ? `after ${after}` : before ? `before ${before}` : 'at a time'
    }
    case 'weekday':
      return describeDays(r.days ?? [])
    case 'sun':
      return SUN_WORDS[r.sun ?? 'down'] ?? 'the sun'
    default: {
      const op = OP_WORDS[r.op ?? 'equals'] ?? r.op
      const field = str(r.field).replace(/^\{\{\s*(.*?)\s*\}\}$/, '$1')
      return `${field} ${op}${['exists', 'not_exists'].includes(r.op ?? '') ? '' : ` ${r.value ?? ''}`}`
    }
  }
}

/** "to is on (+1)", for If and Filter; "a condition holds" when unset. */
function describeCondition(config: Record<string, unknown>): string {
  const short = (t: string) => (t.length > 44 ? `${t.slice(0, 44)}…` : t)
  if (config.mode === 'expression') return str(config.expression) ? short(str(config.expression)) : 'a condition holds'
  if (config.mode === 'jsonata') return str(config.jsonata) ? short(str(config.jsonata)) : 'a condition holds'
  const rules = Array.isArray(config.rules) ? (config.rules as Rule[]) : []
  const r = rules.find((x) => (x.kind && x.kind !== 'value') || str(x.field))
  if (!r) return 'a condition holds'
  const more = rules.length > 1 ? ` (${config.match === 'any' ? 'or' : 'and'} ${rules.length - 1} more)` : ''
  return `${describeRule(r)}${more}`
}

/** 5 + "minutes" -> "5 minutes", 1 + "hours" -> "1 hour". */
export function describeDuration(amount: unknown, unit: unknown): string {
  const n = typeof amount === 'number' ? String(amount) : str(amount)
  const u = str(unit) || 'minutes'
  return `${n || '?'} ${n === '1' ? u.replace(/s$/, '') : u}`
}

/** notify.mobile_app_pixel_7 -> "pixel 7", notify.persistent_notification -> "Home Assistant". */
export function notifyTarget(service: string): string {
  if (service === 'notify.persistent_notification' || service === 'persistent_notification') return 'Home Assistant'
  return service.replace(/^notify\./, '').replace(/^mobile_app_/, '').replace(/_/g, ' ')
}

/** light.turn_on -> "Turn on", input_number.set_value -> "Set value of". */
export function actionWords(action: string): string {
  const service = action.split('.')[1] ?? action
  const words = service.replace(/_/g, ' ')
  const text = words.charAt(0).toUpperCase() + words.slice(1)
  return service.startsWith('set_') ? `${text} of` : text
}

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
    case 'trigger.mqtt': {
      const topic = str(config.topic)
      if (!topic) return 'Choose a topic to listen on'
      const payload = typeof config.payload === 'string' && config.payload ? ` says ${config.payload}` : ''
      return payload ? `${topic}${payload}` : `A message arrives on ${topic}`
    }
    case 'ha.action': {
      const action = str(config.action)
      if (!action) return 'Choose an action'
      const ids = str(config.entity_id)
        .split(/[\s,]+/)
        .filter(Boolean)
      if (!ids.length) return `Run ${action}`
      const first = ids[0].includes('{{') ? ids[0] : entityName(ids[0], entities)
      return `${actionWords(action)} ${first}${ids.length > 1 ? ` and ${ids.length - 1} more` : ''}`
    }
    case 'flow.if':
      return `If ${describeCondition(config)}`
    case 'flow.delay':
      return `Wait ${describeDuration(config.amount, config.unit)}`
    case 'flow.wait': {
      const id = str(config.entity_id)
      if (!id) return 'Pick what to wait for'
      const state = str(config.state)
      return `Wait until ${entityName(id, entities)} is ${state || '…'}`
    }
    case 'push.send': {
      const t = str(config.message)
      const to = str(config.to) || 'all'
      const who = to === 'all' ? 'all browsers' : 'one browser'
      return t ? `Push to ${who}: “${t.length > 32 ? `${t.slice(0, 32)}…` : t}”` : `Push to ${who}`
    }
    case 'ha.notify': {
      const service = str(config.service)
      if (!service) return 'Choose who to notify'
      const t = str(config.message)
      const msg = t ? `: “${t.length > 32 ? `${t.slice(0, 32)}…` : t}”` : ''
      return `Notify ${notifyTarget(service)}${msg}`
    }
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
      const cond = describeCondition(config)
      return cond === 'a condition holds' ? 'Choose what to keep' : `Keep items where ${cond}`
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
    case 'trigger.mqtt':
      return 'inbox'
    case 'ha.action':
      return 'home'
    case 'ha.notify':
      return 'bell'
    case 'push.send':
      return 'browser'
    case 'flow.if':
      return 'branch'
    case 'flow.delay':
      return 'timer'
    case 'flow.wait':
      return 'hourglass'
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
