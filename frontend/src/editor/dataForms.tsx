// Forms for the HTTP and Data steps: request, download, parse, filter, map, compose.

import type { ReactNode } from 'react'
import { Icon } from '../components/Icon.tsx'
import { FieldBlock, type InsertChip, MoreOptions, TemplateText } from './fields.tsx'

type Cfg = Record<string, unknown>
type Set = (key: string, value: unknown) => void
type KV = Record<'key' | 'value', string>

const str = (v: unknown) => (typeof v === 'string' ? v : v == null ? '' : String(v))
const list = <T,>(v: unknown): T[] => (Array.isArray(v) ? (v as T[]) : [])

/** Rows of two inputs with add and remove, stored as `[{ [a]: .., [b]: .. }]`. */
function PairList<K extends string>({
  rows,
  keys,
  placeholders,
  onChange,
  addLabel,
  mono = [true, true],
}: {
  rows: Record<K, string>[]
  keys: [K, K]
  placeholders: [string, string]
  onChange: (rows: Record<K, string>[]) => void
  addLabel: string
  mono?: [boolean, boolean]
}) {
  const update = (i: number, k: K, v: string) => onChange(rows.map((r, j) => (j === i ? { ...r, [k]: v } : r)))
  return (
    <div className="pairs">
      {rows.map((r, i) => (
        <div key={i} className="pair">
          {keys.map((k, n) => (
            <input
              key={k}
              className={`input${mono[n] ? ' mono' : ''}`}
              value={r[k] ?? ''}
              placeholder={placeholders[n]}
              spellCheck={false}
              onChange={(e) => update(i, k, e.target.value)}
            />
          ))}
          <button className="btn ghost icon" aria-label="Remove" onClick={() => onChange(rows.filter((_, j) => j !== i))}>
            <Icon name="x" size={16} />
          </button>
        </div>
      ))}
      <button
        className="btn ghost small add-row"
        onClick={() => onChange([...rows, Object.fromEntries(keys.map((k) => [k, ''])) as Record<K, string>])}
      >
        <Icon name="plus" size={14} /> {addLabel}
      </button>
    </div>
  )
}

function Chips({ options, value, onChange }: { options: [string, string][]; value: string; onChange: (v: string) => void }) {
  return (
    <div className="chips">
      {options.map(([v, label]) => (
        <button key={v} className={`chip${value === v ? ' on' : ''}`} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  )
}

const PATH_HELP = (
  <>
    A JSONPath such as <code>$.rss.channel.item</code>. Tip: after a run, click a key in the Input tab to copy its path.
  </>
)

const AUTH: [string, string][] = [
  ['none', 'None'],
  ['bearer', 'Bearer token'],
  ['basic', 'Username & password'],
  ['header', 'API key header'],
]

/** Sign-in options shared by the HTTP steps. */
function AuthFields({ cfg, set }: { cfg: Cfg; set: Set }) {
  const auth = (cfg.auth ?? { type: 'none' }) as Record<string, string>
  const setAuth = (k: string, v: string) => set('auth', { ...auth, [k]: v })
  const type = auth.type || 'none'
  return (
    <>
      <FieldBlock label="Sign in">
        <Chips options={AUTH} value={type} onChange={(v) => setAuth('type', v)} />
      </FieldBlock>
      {type === 'bearer' && (
        <FieldBlock label="Token">
          <input className="input mono" type="password" value={auth.token ?? ''} onChange={(e) => setAuth('token', e.target.value)} />
        </FieldBlock>
      )}
      {type === 'basic' && (
        <div className="row-fields">
          <FieldBlock label="Username">
            <input className="input" value={auth.username ?? ''} onChange={(e) => setAuth('username', e.target.value)} />
          </FieldBlock>
          <FieldBlock label="Password">
            <input className="input" type="password" value={auth.password ?? ''} onChange={(e) => setAuth('password', e.target.value)} />
          </FieldBlock>
        </div>
      )}
      {type === 'header' && (
        <div className="row-fields">
          <FieldBlock label="Header">
            <input className="input mono" value={auth.header ?? ''} placeholder="X-API-Key" onChange={(e) => setAuth('header', e.target.value)} />
          </FieldBlock>
          <FieldBlock label="Key">
            <input className="input mono" type="password" value={auth.value ?? ''} onChange={(e) => setAuth('value', e.target.value)} />
          </FieldBlock>
        </div>
      )}
    </>
  )
}

/** Query, headers, timeout, size limit and redirects, under More options. */
function HttpExtras({ cfg, set, maxMb, children }: { cfg: Cfg; set: Set; maxMb: number; children?: ReactNode }) {
  return (
    <MoreOptions>
      <FieldBlock label="Query parameters">
        <PairList
          rows={list<KV>(cfg.query)}
          keys={['key', 'value']}
          placeholders={['name', 'value']}
          onChange={(r) => set('query', r)}
          addLabel="Add parameter"
        />
      </FieldBlock>
      <FieldBlock label="Headers">
        <PairList
          rows={list<KV>(cfg.headers)}
          keys={['key', 'value']}
          placeholders={['Accept', 'application/json']}
          onChange={(r) => set('headers', r)}
          addLabel="Add header"
        />
      </FieldBlock>
      <div className="row-fields">
        <FieldBlock label="Give up after (s)">
          <input className="input narrow" type="number" min={1} max={600} value={str(cfg.timeout_s ?? 30)} onChange={(e) => set('timeout_s', Number(e.target.value))} />
        </FieldBlock>
        <FieldBlock label="Size limit (MB)">
          <input className="input narrow" type="number" min={1} max={1024} value={str(cfg.max_mb ?? maxMb)} onChange={(e) => set('max_mb', Number(e.target.value))} />
        </FieldBlock>
      </div>
      <div className="field toggle-row">
        <button className="switch" role="switch" aria-checked={cfg.follow_redirects !== false} onClick={() => set('follow_redirects', cfg.follow_redirects === false)} />
        <span>Follow redirects</span>
      </div>
      {children}
    </MoreOptions>
  )
}

export function DownloadForm({ cfg, set, chips }: { cfg: Cfg; set: Set; chips: InsertChip[] }) {
  return (
    <>
      <FieldBlock label="Address" help="A web address that returns a feed, file or API response.">
        <TemplateText value={str(cfg.url)} onChange={(v) => set('url', v)} placeholder="https://example.com/feed.xml" chips={chips} />
      </FieldBlock>
      <AuthFields cfg={cfg} set={set} />
      <HttpExtras cfg={cfg} set={set} maxMb={25} />
    </>
  )
}

const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
const BODY_TYPES: [string, string][] = [
  ['none', 'None'],
  ['json', 'JSON'],
  ['form', 'Form'],
  ['text', 'Text'],
]

export function RequestForm({ cfg, set, chips }: { cfg: Cfg; set: Set; chips: InsertChip[] }) {
  const method = str(cfg.method) || 'POST'
  const bodyType = str(cfg.body_type) || 'none'
  const noBody = method === 'GET' || method === 'HEAD'
  return (
    <>
      <FieldBlock label="Method">
        <Chips options={METHODS.map((m) => [m, m])} value={method} onChange={(v) => set('method', v)} />
      </FieldBlock>
      <FieldBlock label="Address">
        <TemplateText
          value={str(cfg.url)}
          onChange={(v) => set('url', v)}
          placeholder="https://api.example.com/v1/items"
          chips={chips.filter((c) => !c.label.startsWith('All input'))}
        />
      </FieldBlock>
      {!noBody && (
        <>
          <FieldBlock label="Body">
            <Chips options={BODY_TYPES} value={bodyType} onChange={(v) => set('body_type', v)} />
          </FieldBlock>
          {bodyType === 'json' && (
            <FieldBlock
              label="JSON"
              help={
                <>
                  Insert text with <code>{'{{ value | tojson }}'}</code> so quotes are escaped. Checked for valid JSON before
                  sending.
                </>
              }
            >
              <TemplateText
                multiline
                lang="json"
                value={str(cfg.body)}
                onChange={(v) => set('body', v)}
                placeholder={'{\n  "title": {{ input.title | tojson }}\n}'}
                chips={chips}
              />
            </FieldBlock>
          )}
          {bodyType === 'form' && (
            <FieldBlock label="Form fields" help="Sent as application/x-www-form-urlencoded. Values can use templates.">
              <PairList
                rows={list<KV>(cfg.form)}
                keys={['key', 'value']}
                placeholders={['name', 'value']}
                onChange={(r) => set('form', r)}
                addLabel="Add field"
              />
            </FieldBlock>
          )}
          {bodyType === 'text' && (
            <>
              <FieldBlock label="Text">
                <TemplateText multiline value={str(cfg.body)} onChange={(v) => set('body', v)} placeholder="Hello from ergo" chips={chips} />
              </FieldBlock>
              <FieldBlock label="Content type">
                <input
                  className="input mono"
                  value={str(cfg.content_type)}
                  placeholder="text/plain"
                  spellCheck={false}
                  onChange={(e) => set('content_type', e.target.value)}
                />
              </FieldBlock>
            </>
          )}
        </>
      )}
      <AuthFields cfg={cfg} set={set} />
      <HttpExtras cfg={cfg} set={set} maxMb={5}>
        <FieldBlock label="Read the response as">
          <Chips
            options={[
              ['auto', 'Detect it'],
              ['json', 'JSON'],
              ['text', 'Text'],
            ]}
            value={str(cfg.response) || 'auto'}
            onChange={(v) => set('response', v)}
          />
        </FieldBlock>
        <div className="field toggle-row">
          <button className="switch" role="switch" aria-checked={cfg.fail_on_status !== false} onClick={() => set('fail_on_status', cfg.fail_on_status === false)} />
          <span>Fail the step on 4xx and 5xx responses</span>
        </div>
      </HttpExtras>
    </>
  )
}

const FORMAT_LABEL: Record<string, string> = { auto: 'Detect it', xml: 'XML', json: 'JSON' }

export function ParseForm({ cfg, set, formats }: { cfg: Cfg; set: Set; formats: string[] }) {
  return (
    <FieldBlock label="Read it as" help="Detect it uses the content type, then looks at the first character.">
      <Chips options={formats.map((f) => [f, FORMAT_LABEL[f] ?? f.toUpperCase()])} value={str(cfg.format) || 'auto'} onChange={(v) => set('format', v)} />
    </FieldBlock>
  )
}

const OPS: [string, string][] = [
  ['equals', 'is'],
  ['not_equals', 'is not'],
  ['contains', 'contains'],
  ['not_contains', 'doesn’t contain'],
  ['starts_with', 'starts with'],
  ['ends_with', 'ends with'],
  ['greater_than', 'is more than'],
  ['less_than', 'is less than'],
  ['exists', 'is set'],
  ['not_exists', 'is not set'],
]

type Rule = { field: string; op: string; value: string }

/** Rules (field, operator, value) with all/any, as Filter and If use them. */
function RulesEditor({
  cfg,
  set,
  fieldPlaceholder,
  valuePlaceholder,
  help,
}: {
  cfg: Cfg
  set: Set
  fieldPlaceholder: string
  valuePlaceholder: string
  help: ReactNode
}) {
  const rules = list<Rule>(cfg.rules)
  const setRule = (i: number, r: Partial<Rule>) => set('rules', rules.map((x, j) => (j === i ? { ...x, ...r } : x)))
  return (
    <>
      {rules.length > 1 && (
        <Chips options={[['all', 'All rules match'], ['any', 'Any rule matches']]} value={str(cfg.match) || 'all'} onChange={(v) => set('match', v)} />
      )}
      <div className="rules">
        {rules.map((r, i) => (
          <div key={i} className="rule">
            <input className="input mono" value={r.field} placeholder={fieldPlaceholder} spellCheck={false} onChange={(e) => setRule(i, { field: e.target.value })} />
            <select className="select" value={r.op || 'equals'} onChange={(e) => setRule(i, { op: e.target.value })}>
              {OPS.map(([v, l]) => (
                <option key={v} value={v}>
                  {l}
                </option>
              ))}
            </select>
            {!['exists', 'not_exists'].includes(r.op) ? (
              <input className="input mono" value={r.value} placeholder={valuePlaceholder} spellCheck={false} onChange={(e) => setRule(i, { value: e.target.value })} />
            ) : (
              <span />
            )}
            <button className="btn ghost icon" aria-label="Remove rule" onClick={() => set('rules', rules.filter((_, j) => j !== i))}>
              <Icon name="x" size={16} />
            </button>
          </div>
        ))}
        <button className="btn ghost small add-row" onClick={() => set('rules', [...rules, { field: '', op: 'equals', value: '' }])}>
          <Icon name="plus" size={14} /> Add rule
        </button>
      </div>
      <p className="help">{help}</p>
    </>
  )
}

/** If: a condition on the step's input, with yes and no exits. */
export function IfForm({ cfg, set, chips }: { cfg: Cfg; set: Set; chips: InsertChip[] }) {
  const mode = str(cfg.mode) || 'rules'
  return (
    <>
      <FieldBlock label="Check with">
        <Chips
          options={[
            ['rules', 'Rules'],
            ['expression', 'Template'],
            ['jsonata', 'JSONata'],
          ]}
          value={mode}
          onChange={(v) => set('mode', v)}
        />
      </FieldBlock>
      {mode === 'rules' ? (
        <RulesEditor
          cfg={cfg}
          set={set}
          fieldPlaceholder="to"
          valuePlaceholder="on"
          help={
            <>
              Fields are keys of this step's input, like <code>to</code> or <code>to_state.attributes.temperature</code>, or a
              template such as <code>{'{{ trigger.time }}'}</code> for anything else. Text matches ignore case; numbers compare
              as numbers.
            </>
          }
        />
      ) : mode === 'jsonata' ? (
        <FieldBlock
          label="Yes when"
          help={
            <>
              Runs on this step's input; <code>$trigger</code> and <code>$steps</code> are there too. Yes when the result is
              true (not empty, 0 or missing).
            </>
          }
        >
          <textarea
            className="textarea mono short"
            value={str(cfg.jsonata)}
            spellCheck={false}
            placeholder={"to = 'on' and to_state.attributes.brightness > 100"}
            onChange={(e) => set('jsonata', e.target.value)}
          />
        </FieldBlock>
      ) : (
        <FieldBlock label="Template" help="Yes when the result is not empty, false or 0.">
          <TemplateText
            value={str(cfg.expression)}
            onChange={(v) => set('expression', v)}
            placeholder="{{ input.to == 'on' and trigger.time[11:] > '18:00' }}"
            chips={chips}
          />
        </FieldBlock>
      )}
      <p className="help">
        The run continues from <strong>yes</strong> or <strong>no</strong>, with the same data this step got.
      </p>
    </>
  )
}

export function FilterForm({ cfg, set }: { cfg: Cfg; set: Set }) {
  const mode = str(cfg.mode) || 'rules'
  return (
    <>
      <FieldBlock label="Which list?" help={PATH_HELP}>
        <input className="input mono" value={str(cfg.path)} placeholder="$.rss.channel.item" spellCheck={false} onChange={(e) => set('path', e.target.value)} />
      </FieldBlock>
      <FieldBlock label="Keep items where">
        <Chips
          options={[
            ['rules', 'Rules'],
            ['expression', 'Template'],
            ['jsonata', 'JSONata'],
          ]}
          value={mode}
          onChange={(v) => set('mode', v)}
        />
      </FieldBlock>
      {mode === 'rules' ? (
        <RulesEditor
          cfg={cfg}
          set={set}
          fieldPlaceholder="category"
          valuePlaceholder="rust"
          help={
            <>
              Fields are keys of each item, like <code>title</code> or <code>author.name</code>. Text matches ignore case.
            </>
          }
        />
      ) : mode === 'jsonata' ? (
        <FieldBlock
          label="Keep an item when"
          help={
            <>
              Runs on each item, so <code>title</code> is the item's title. <code>$input</code> is the whole input;{' '}
              <code>$trigger</code> and <code>$steps</code> are there too. Kept when the result is true (not empty, 0 or
              missing).{' '}
              <a href="https://docs.jsonata.org/overview" target="_blank" rel="noreferrer">
                JSONata docs
              </a>
            </>
          }
        >
          <textarea
            className="textarea mono short"
            value={str(cfg.jsonata)}
            spellCheck={false}
            placeholder={"category = 'rust' and $contains(title, 'async')"}
            onChange={(e) => set('jsonata', e.target.value)}
          />
        </FieldBlock>
      ) : (
        <FieldBlock label="Template" help="Kept when the result is not empty, false or 0.">
          <TemplateText
            value={str(cfg.expression)}
            onChange={(v) => set('expression', v)}
            placeholder="{{ 'rust' in item.title | lower }}"
            chips={[{ label: 'Item', value: '{{ item }}', title: 'The item being checked' }]}
          />
        </FieldBlock>
      )}
      <p className="help">When nothing is kept the step leaves by its <strong>empty</strong> exit instead.</p>
    </>
  )
}

export function MapForm({ cfg, set }: { cfg: Cfg; set: Set }) {
  const mode = str(cfg.mode) || 'fields'
  return (
    <>
      <FieldBlock label="How">
        <Chips options={[['fields', 'Pick fields'], ['jsonata', 'JSONata']]} value={mode} onChange={(v) => set('mode', v)} />
      </FieldBlock>
      {mode === 'jsonata' ? (
        <FieldBlock
          label="Expression"
          help={
            <>
              Runs on this step's input. Use <code>$trigger</code> and <code>$steps.&lt;id&gt;.output</code> for earlier data. A
              list comes out as <code>items</code> with a <code>count</code>. Try expressions in the{' '}
              <a href="https://try.jsonata.org" target="_blank" rel="noreferrer">JSONata exerciser</a>.
            </>
          }
        >
          <textarea
            className="textarea mono tall"
            value={str(cfg.expression)}
            spellCheck={false}
            placeholder={'rss.channel.item[category = "rust"].{\n  "title": title,\n  "link": link\n}'}
            onChange={(e) => set('expression', e.target.value)}
          />
        </FieldBlock>
      ) : (
        <>
          <FieldBlock label="From" help={<>A list is reshaped item by item. {PATH_HELP}</>}>
            <input className="input mono" value={str(cfg.path)} placeholder="$.items" spellCheck={false} onChange={(e) => set('path', e.target.value)} />
          </FieldBlock>
          <FieldBlock
            label="Fields"
            help={
              <>
                A value starting with <code>$</code> is a path in the item; anything else is a template with <code>{'{{ item }}'}</code>. Use <code>a.b</code> names to nest.
              </>
            }
          >
            <PairList
              rows={list<Record<'name' | 'value', string>>(cfg.fields)}
              keys={['name', 'value']}
              placeholders={['title', '$.title']}
              onChange={(r) => set('fields', r)}
              addLabel="Add field"
              mono={[false, true]}
            />
          </FieldBlock>
        </>
      )}
    </>
  )
}

const COMPOSE_FORMATS: [string, string][] = [
  ['plain', 'Plain text'],
  ['markdown', 'Markdown'],
  ['json', 'JSON'],
]

export function ComposeForm({ cfg, set, chips }: { cfg: Cfg; set: Set; chips: InsertChip[] }) {
  return (
    <>
      <FieldBlock label="Message">
        <TemplateText
          multiline
          value={str(cfg.template)}
          onChange={(v) => set('template', v)}
          placeholder={'New post: {{ input.items[0].title }}'}
          chips={chips}
          lang={str(cfg.format) === 'json' ? 'json' : 'text'}
        />
      </FieldBlock>
      <FieldBlock label="Format" help={str(cfg.format) === 'json' ? 'The message must be valid JSON; later steps also get it parsed as input.json.' : undefined}>
        <Chips options={COMPOSE_FORMATS} value={str(cfg.format) || 'plain'} onChange={(v) => set('format', v)} />
      </FieldBlock>
    </>
  )
}
