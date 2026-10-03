import { useCallback, useEffect, useState } from 'react'
import { api, type DbPage, type DbTable } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { PageHeader } from '../components/PageHeader.tsx'
import { TopBar } from '../components/TopBar.tsx'
import './Database.css'

const ABOUT: Record<string, string> = {
  workflows: 'Your workflows, with their current draft',
  workflow_versions: 'Every version you made live',
  runs: 'Each time a workflow ran',
  run_nodes: 'What each step did in each run',
  meta: 'Internal settings',
  push_subscriptions: 'Browsers that get web push notifications',
}

const PAGE_SIZES = [25, 50, 100]

/** Parses JSON text so it can be shown pretty; anything else is left as is. */
function asJson(v: unknown): unknown | undefined {
  if (typeof v !== 'string' || !/^[[{]/.test(v.trim())) return undefined
  try {
    return JSON.parse(v)
  } catch {
    return undefined
  }
}

function Cell({ value }: { value: unknown }) {
  if (value === null) return <span className="cell-null">null</span>
  if (typeof value === 'number') return <span className="cell-num">{value}</span>
  const text = String(value)
  const json = asJson(value)
  return <span className={json !== undefined ? 'cell-json' : undefined}>{text.length > 120 ? `${text.slice(0, 120)}…` : text}</span>
}

function Value({ value }: { value: unknown }) {
  if (value === null) return <span className="cell-null">null</span>
  const json = asJson(value)
  if (json !== undefined) return <pre className="json">{JSON.stringify(json, null, 2)}</pre>
  return <div className="value-text">{String(value)}</div>
}

export function Database() {
  const [tables, setTables] = useState<DbTable[]>([])
  const [dbPath, setDbPath] = useState('')
  const [retention, setRetention] = useState<{ days: number; max_runs: number } | null>(null)
  const [table, setTable] = useState<string | null>(null)
  const [page, setPage] = useState<DbPage | null>(null)
  const [offset, setOffset] = useState(0)
  const [pageSize, setPageSize] = useState(50)
  const [row, setRow] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const loadTables = useCallback(async () => {
    try {
      const r = await api.dbTables()
      setTables(r.tables)
      setDbPath(r.path)
      setRetention(r.retention)
      setTable((t) => t ?? r.tables.find((x) => x.name === 'workflows')?.name ?? r.tables[0]?.name ?? null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [])

  const loadRows = useCallback(async () => {
    if (!table) return
    setLoading(true)
    try {
      setPage(await api.dbRows(table, offset, pageSize))
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [table, offset, pageSize])

  // Loading data from the backend is what these effects are for.
  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    loadTables()
  }, [loadTables])

  useEffect(() => {
    // oxlint-disable-next-line react/set-state-in-effect
    loadRows()
  }, [loadRows])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setRow(null)
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const pick = (name: string) => {
    setTable(name)
    setOffset(0)
    setRow(null)
    setPage(null)
  }

  const refresh = () => {
    loadTables()
    loadRows()
  }

  const selected = page && row !== null ? page.rows[row] : null
  const last = page ? Math.min(page.offset + page.rows.length, page.total) : 0
  const pageCount = page ? Math.max(1, Math.ceil(page.total / pageSize)) : 1
  const pageNo = Math.floor(offset / pageSize) + 1
  const goTo = (n: number) => {
    setOffset((Math.min(Math.max(n, 1), pageCount) - 1) * pageSize)
    setRow(null)
  }

  return (
    <>
      <TopBar section="data" />
      <main className="page db-page">
        <div className="page-inner db-inner">
        <PageHeader
          title="Database"
          subtitle={
            <>
              Everything ergo has stored, newest first <span className="faint">·</span>{' '}
              <span className="mono faint">{dbPath}</span>
            </>
          }
          actions={
            <>
              <span className="readonly-pill">
                <Icon name="check" size={14} /> Read-only
              </span>
              <button className="btn" onClick={refresh}>
                <Icon name="history" size={16} /> Refresh
              </button>
            </>
          }
        />
        {error && <div className="issue">{error}</div>}

        <div className={`db-body${selected ? ' with-row' : ''}`}>
          <nav className="surface db-tables" aria-label="Tables">
            {tables.map((t) => (
              <button key={t.name} className={`db-table${t.name === table ? ' on' : ''}`} onClick={() => pick(t.name)}>
                <span className="db-table-name mono">{t.name}</span>
                <span className="db-table-count">{t.rows}</span>
                <span className="db-table-about">{ABOUT[t.name] ?? `${t.columns.length} columns`}</span>
              </button>
            ))}
          </nav>

          <section className="surface db-data">
            {page && (
              <>
                <header className="db-data-head">
                  <h2 className="mono">{page.table}</h2>
                  <p className="quiet">
                    {page.total === 0
                      ? 'No rows yet.'
                      : `${page.total} ${page.total === 1 ? 'row' : 'rows'}. Click a row to see everything in it.`}
                    {retention && (page.table === 'runs' || page.table === 'run_nodes') && (
                      <span className="faint">
                        {' '}
                        Kept for {retention.days} {retention.days === 1 ? 'day' : 'days'}, at most{' '}
                        {retention.max_runs.toLocaleString()} runs; older runs are deleted automatically.
                      </span>
                    )}
                  </p>
                </header>
                <div className="grid-wrap">
                  {page.rows.length > 0 && (
                    <table className="grid">
                      <thead>
                        <tr>
                          {page.columns.map((c) => (
                            <th key={c} className="mono">
                              {c}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {page.rows.map((r, i) => (
                          <tr key={i} className={row === i ? 'on' : ''} onClick={() => setRow(i)}>
                            {r.map((v, j) => (
                              <td key={j}>
                                <Cell value={v} />
                              </td>
                            ))}
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}
                </div>
                <footer className="pagination">
                  <label className="page-size">
                    Rows per page
                    <select
                      className="select"
                      value={pageSize}
                      onChange={(e) => {
                        setPageSize(Number(e.target.value))
                        setOffset(0)
                        setRow(null)
                      }}
                    >
                      {PAGE_SIZES.map((n) => (
                        <option key={n} value={n}>
                          {n}
                        </option>
                      ))}
                    </select>
                  </label>
                  <span className="quiet">{page.total ? `${page.offset + 1}–${last} of ${page.total}` : '0 of 0'}</span>
                  <span className="spacer" />
                  <span className="quiet">
                    Page {pageNo} of {pageCount}
                  </span>
                  <div className="page-buttons">
                    <button className="btn" disabled={pageNo <= 1 || loading} onClick={() => goTo(1)}>
                      First
                    </button>
                    <button className="btn" disabled={pageNo <= 1 || loading} onClick={() => goTo(pageNo - 1)}>
                      <Icon name="back" size={15} /> Previous
                    </button>
                    <button className="btn" disabled={pageNo >= pageCount || loading} onClick={() => goTo(pageNo + 1)}>
                      Next <Icon name="back" size={15} className="flip" />
                    </button>
                    <button className="btn" disabled={pageNo >= pageCount || loading} onClick={() => goTo(pageCount)}>
                      Last
                    </button>
                  </div>
                </footer>
              </>
            )}
          </section>

          {selected && page && (
            <aside className="surface db-row">
              <header className="drawer-head">
                <span className="step-icon plain">
                  <Icon name="dots" size={22} />
                </span>
                <div>
                  <div className="step-kicker">{page.table}</div>
                  <h2>Row {page.offset + (row ?? 0) + 1}</h2>
                </div>
                <button className="btn ghost icon" onClick={() => setRow(null)} aria-label="Close">
                  <Icon name="x" />
                </button>
              </header>
              <div className="drawer-body">
                {page.columns.map((c, i) => (
                  <div key={c} className="field">
                    <div className="label mono">{c}</div>
                    <Value value={selected[i]} />
                  </div>
                ))}
              </div>
            </aside>
          )}
        </div>
        </div>
      </main>
    </>
  )
}
