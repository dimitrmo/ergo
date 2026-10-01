// Types and calls for the ergo REST API. Paths are relative so the app works
// under Home Assistant's Ingress prefix.

export type NodeKind = 'trigger' | 'action' | 'transform' | 'data' | 'flow'

export type FieldType =
  | { type: 'text' }
  | { type: 'template' }
  | { type: 'entity' }
  | { type: 'cron' }
  | { type: 'select'; options: string[] }
  | { type: 'bool' }
  | { type: 'number' }

export type Field = FieldType & {
  key: string
  label: string
  required: boolean
  default?: unknown
  help?: string
  placeholder?: string
}

/** A (small) JSON Schema, as node types declare their input and output. */
export interface JsonSchema {
  type?: string
  description?: string
  properties?: Record<string, JsonSchema>
}

export interface NodeSchema {
  type: string
  kind: NodeKind
  title: string
  description: string
  fields: Field[]
  ports: string[]
  input: JsonSchema
  output: JsonSchema
}

export interface GraphNode {
  id: string
  type: string
  label?: string
  config: Record<string, unknown>
  position: { x: number; y: number }
}

export interface GraphEdge {
  from: string
  fromPort: string
  to: string
}

export interface Graph {
  schema_version: number
  mode: 'single'
  nodes: GraphNode[]
  edges: GraphEdge[]
}

export interface LastRun {
  id: string
  status: RunStatus
  started_at: string
}

export interface Workflow {
  id: string
  name: string
  enabled: boolean
  draft: Graph
  active_version: number | null
  dirty: boolean
  manual_trigger: string | null
  created_at: string
  updated_at: string
  last_run: LastRun | null
}

export interface Issue {
  severity: 'error' | 'warning'
  node?: string
  message: string
}

export interface WorkflowResponse {
  workflow: Workflow
  issues: Issue[]
}

export type RunStatus = 'running' | 'success' | 'failed' | 'skipped' | 'interrupted'

export interface Run {
  id: string
  workflow_id: string
  version: number
  trigger_node: string
  trigger: Record<string, unknown>
  status: RunStatus
  error: string | null
  started_at: string
  finished_at: string | null
}

export interface StepError {
  kind: 'timeout' | 'http_status' | 'network' | 'parse' | 'template' | 'ha' | 'mqtt' | 'config' | 'other'
  message: string
  details?: unknown
}

export interface Attempt {
  started_at: string
  duration_ms: number
  error?: StepError
}

/** One step of a run: what it got, what it did, what it produced. */
export interface RunNode {
  seq: number
  node_id: string
  node_type: string
  port: string | null
  input: unknown
  /** The step's config after templates were rendered. */
  config: unknown
  output: unknown
  error: StepError | null
  attempts: Attempt[]
  logs: string[]
  started_at: string
  duration_ms: number
}

export interface RunDetail {
  run: Run
  nodes: RunNode[]
}

export type StartOutcome =
  | { outcome: 'started'; run_id: string }
  | { outcome: 'skipped'; run_id: string; reason: string }

export interface Entity {
  entity_id: string
  name: string
  state: string
  domain: string
  unit?: string
  options?: string[]
}

export interface Ready {
  status: 'ok' | 'degraded'
  version: string
  uptime_s: number
  checks: {
    ha_websocket: { ok: boolean; error: string | null; last_event_at: string | null; ha_version: string | null }
    mqtt: { ok: boolean; configured: boolean; connected: boolean; broker: string | null; error: string | null }
    database: { ok: boolean; error: string | null }
    scheduler: { ok: boolean; time_zone: string }
  }
}

export interface DbColumn {
  name: string
  type: string
  pk: boolean
}

export interface DbTable {
  name: string
  rows: number
  columns: DbColumn[]
}

export interface DbPage {
  table: string
  columns: string[]
  rows: unknown[][]
  total: number
  offset: number
  limit: number
}

/** An export file: workflows' names and drafts, for Import on another ergo. */
export interface ExportFile {
  format: 'ergo.workflows'
  version: number
  exported_at: string
  ergo_version: string
  workflows: { name: string; draft: Graph }[]
}

export class ApiError extends Error {
  status: number
  issues: Issue[]
  constructor(status: number, message: string, issues: Issue[] = []) {
    super(message)
    this.status = status
    this.issues = issues
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const data = await res.json().catch(() => ({}))
  // /ready answers 503 with a full body when degraded; that's data, not an error.
  if (!res.ok && !(path === 'ready' && res.status === 503)) {
    throw new ApiError(res.status, data.error ?? res.statusText, data.issues ?? [])
  }
  return data as T
}

export const api = {
  ready: () => call<Ready>('GET', 'ready'),
  workflows: () => call<Workflow[]>('GET', 'api/workflows'),
  workflow: (id: string) => call<WorkflowResponse>('GET', `api/workflows/${id}`),
  createWorkflow: (name: string, draft?: Graph) =>
    call<WorkflowResponse>('POST', 'api/workflows', { name, draft }),
  saveWorkflow: (id: string, patch: { name?: string; draft?: Graph }) =>
    call<WorkflowResponse>('PUT', `api/workflows/${id}`, patch),
  deleteWorkflow: (id: string) => call<unknown>('DELETE', `api/workflows/${id}`),
  activate: (id: string) => call<WorkflowResponse>('POST', `api/workflows/${id}/activate`),
  setEnabled: (id: string, enabled: boolean) =>
    call<WorkflowResponse>('POST', `api/workflows/${id}/${enabled ? 'enable' : 'disable'}`),
  run: (id: string, opts: { draft?: boolean; node?: string } = {}) =>
    call<StartOutcome>('POST', `api/workflows/${id}/run`, opts),
  exportWorkflows: (ids?: string[]) =>
    call<ExportFile>('GET', `api/export${ids ? `?ids=${ids.map(encodeURIComponent).join(',')}` : ''}`),
  importWorkflows: (file: unknown) =>
    call<{ created: { id: string; name: string }[] }>('POST', 'api/import', file),
  runs: (workflowId?: string, limit = 50) =>
    call<Run[]>('GET', `api/runs?limit=${limit}${workflowId ? `&workflow=${workflowId}` : ''}`),
  runDetail: (id: string) => call<RunDetail>('GET', `api/runs/${id}`),
  nodes: () => call<NodeSchema[]>('GET', 'api/nodes'),
  entities: () => call<Entity[]>('GET', 'api/entities'),
  dbTables: () =>
    call<{ path: string; tables: DbTable[]; retention: { days: number; max_runs: number } }>('GET', 'api/db/tables'),
  dbRows: (table: string, offset = 0, limit = 50) =>
    call<DbPage>('GET', `api/db/tables/${encodeURIComponent(table)}?offset=${offset}&limit=${limit}`),
  cronPreview: (cron: string) =>
    call<{ next: string[]; time_zone: string }>('POST', 'api/cron/preview', { cron }),
}

/** Saves an export file as a download, e.g. ergo-heater-2026-10-01.json. */
export function downloadExport(file: ExportFile, label: string) {
  const slug = label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'workflows'
  const url = URL.createObjectURL(new Blob([JSON.stringify(file, null, 2)], { type: 'application/json' }))
  const a = document.createElement('a')
  a.href = url
  a.download = `ergo-${slug}-${file.exported_at.slice(0, 10)}.json`
  a.click()
  URL.revokeObjectURL(url)
}

export function emptyGraph(): Graph {
  return { schema_version: 1, mode: 'single', nodes: [], edges: [] }
}

/** "3 min ago", "2 h ago", or a date for anything older than a day. */
export function ago(iso: string | null | undefined): string {
  if (!iso) return '—'
  const s = Math.max(0, (Date.now() - Date.parse(iso)) / 1000)
  if (s < 60) return `${Math.floor(s)} s ago`
  if (s < 3600) return `${Math.floor(s / 60)} min ago`
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`
  return new Date(iso).toLocaleDateString()
}

export function clock(iso: string | null | undefined): string {
  if (!iso) return '—'
  return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

export function runDuration(run: Run): string {
  if (!run.finished_at) return run.status === 'running' ? '…' : '—'
  const ms = Date.parse(run.finished_at) - Date.parse(run.started_at)
  return ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`
}
