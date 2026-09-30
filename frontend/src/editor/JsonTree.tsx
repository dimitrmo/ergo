import { useState } from 'react'

/** `$.items[0].title`; keys that aren't plain identifiers use `['key']`. */
function childPath(parent: string, key: string | number): string {
  if (typeof key === 'number') return `${parent}[${key}]`
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(key) ? `${parent}.${key}` : `${parent}['${key.replace(/'/g, "\\'")}']`
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    // Clipboard API can be blocked inside HA's Ingress iframe; fall back.
    const t = document.createElement('textarea')
    t.value = text
    document.body.appendChild(t)
    t.select()
    document.execCommand('copy')
    t.remove()
  }
}

function Scalar({ value }: { value: unknown }) {
  if (value === null) return <span className="jt-null">null</span>
  if (typeof value === 'string') return <span className="jt-string">"{value}"</span>
  if (typeof value === 'number') return <span className="jt-number">{value}</span>
  if (typeof value === 'boolean') return <span className="jt-bool">{String(value)}</span>
  return <span>{String(value)}</span>
}

function Row({
  name,
  value,
  path,
  depth,
  onCopied,
}: {
  name: string | number | null
  value: unknown
  path: string
  depth: number
  onCopied: (path: string) => void
}) {
  const isObj = value !== null && typeof value === 'object'
  const entries: [string | number, unknown][] = isObj
    ? Array.isArray(value)
      ? value.map((v, i) => [i, v])
      : Object.entries(value as Record<string, unknown>)
    : []
  const [open, setOpen] = useState(depth < 2)
  const label =
    name === null ? null : (
      <button
        className="jt-key"
        title={`Copy ${path}`}
        onClick={() => {
          copy(path)
          onCopied(path)
        }}
      >
        {typeof name === 'number' ? `[${name}]` : name}
      </button>
    )

  if (!isObj) {
    return (
      <div className="jt-row" style={{ paddingLeft: depth * 14 }}>
        {label}
        {label && <span className="jt-colon">: </span>}
        <Scalar value={value} />
      </div>
    )
  }
  const summary = Array.isArray(value) ? `[ ${entries.length} ]` : `{ ${entries.length} }`
  return (
    <>
      <div className="jt-row" style={{ paddingLeft: depth * 14 }}>
        <button className={`jt-toggle${open ? ' open' : ''}`} onClick={() => setOpen(!open)} aria-label={open ? 'Collapse' : 'Expand'}>
          ▸
        </button>
        {label}
        {label && <span className="jt-colon">: </span>}
        {!open && <span className="jt-summary">{summary}</span>}
        {open && entries.length === 0 && <span className="jt-summary">{summary}</span>}
      </div>
      {open &&
        entries.map(([k, v]) => (
          <Row key={String(k)} name={k} value={v} path={childPath(path, k)} depth={depth + 1} onCopied={onCopied} />
        ))}
    </>
  )
}

/** Collapsible JSON; clicking a key copies its JSONPath. */
export function JsonTree({ value }: { value: unknown }) {
  const [copied, setCopied] = useState<string | null>(null)
  if (value === undefined) return <div className="jt-empty">Nothing</div>
  const truncated =
    value !== null && typeof value === 'object' && (value as { truncated?: boolean }).truncated === true
  return (
    <div className="json-tree">
      {truncated && (
        <div className="jt-note">
          Cut to 256 KB for storage; the full value was {(value as { size: number }).size.toLocaleString()} bytes.
        </div>
      )}
      <Row name={null} value={value} path="$" depth={0} onCopied={(p) => {
        setCopied(p)
        setTimeout(() => setCopied((c) => (c === p ? null : c)), 1800)
      }} />
      {copied && <div className="jt-copied">Copied {copied}</div>}
    </div>
  )
}
