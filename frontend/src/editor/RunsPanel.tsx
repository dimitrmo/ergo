import { ago, clock, type Run, runDuration } from '../api.ts'
import { Icon } from '../components/Icon.tsx'

const STATUS: Record<string, string> = {
  success: 'Worked',
  failed: 'Failed',
  skipped: 'Skipped',
  running: 'Running…',
  interrupted: 'Interrupted',
}

function why(run: Run): string {
  const t = run.trigger as { kind?: string; id?: string | null; entity_id?: string; to?: string; simulated?: boolean }
  if (t.simulated || run.version === 0) return 'You pressed Try it'
  if (t.kind === 'state') return `${t.entity_id} turned ${t.to ?? '?'}`
  if (t.kind === 'cron') return `Scheduled${t.id ? ` (${t.id})` : ''}`
  return 'Started by hand'
}

export function History({
  runs,
  shownRunId,
  onShow,
  onClose,
}: {
  runs: Run[]
  shownRunId: string | null
  onShow: (id: string) => void
  onClose: () => void
}) {
  return (
    <aside className="drawer">
      <header className="drawer-head">
        <span className="step-icon plain">
          <Icon name="history" size={22} />
        </span>
        <div>
          <div className="step-kicker">Last {runs.length} runs</div>
          <h2>History</h2>
        </div>
        <button className="btn ghost icon" onClick={onClose} aria-label="Close">
          <Icon name="x" />
        </button>
      </header>
      <div className="drawer-body flush">
        {!runs.length && <p className="quiet pad">Nothing yet. Press Try it, or go live and let it run.</p>}
        {runs.map((r) => (
          <button key={r.id} className={`run-row${r.id === shownRunId ? ' active' : ''}`} onClick={() => onShow(r.id)}>
            <span className={`dot ${r.status}`} />
            <span className="run-main">
              <span className="run-status">{STATUS[r.status] ?? r.status}</span>
              <span className="run-why">{why(r)}</span>
              {r.error && <span className={`run-error ${r.status}`}>{r.error}</span>}
            </span>
            <span className="run-when">
              <span>{clock(r.started_at)}</span>
              <span className="faint">
                {ago(r.started_at)} · {runDuration(r)}
              </span>
            </span>
          </button>
        ))}
      </div>
      <footer className="drawer-foot faint">Pick a run to see what each step did on the canvas.</footer>
    </aside>
  )
}
