import { useEffect, useRef, useState } from 'react'
import { ago, api, downloadExport, type Workflow } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { Confirm } from '../components/Confirm.tsx'
import { PageHeader } from '../components/PageHeader.tsx'
import { TopBar } from '../components/TopBar.tsx'
import { describeWorkflow, nodeIcon } from '../describe.ts'
import { useEntities } from '../hooks.ts'
import './Workflows.css'

const STATUS_TEXT: Record<string, string> = {
  success: 'Worked',
  failed: 'Failed',
  skipped: 'Skipped',
  running: 'Running',
  interrupted: 'Interrupted',
}

type RunState = { phase: 'running' } | { phase: 'done'; ok: boolean; text: string }

export function Workflows() {
  const [running, setRunning] = useState<Record<string, RunState>>({})
  const [confirming, setConfirming] = useState<Workflow | null>(null)
  const [deleting, setDeleting] = useState<Workflow | null>(null)
  const setRun = (id: string, state: RunState | null) =>
    setRunning((r) => {
      const next = { ...r }
      if (state) next[id] = state
      else delete next[id]
      return next
    })

  const runNow = async (wf: Workflow) => {
    setRun(wf.id, { phase: 'running' })
    try {
      const outcome = await api.run(wf.id, { node: wf.manual_trigger ?? undefined })
      if (outcome.outcome === 'skipped') {
        setRun(wf.id, { phase: 'done', ok: false, text: `Skipped: ${outcome.reason}` })
      } else {
        let detail = await api.runDetail(outcome.run_id)
        for (let i = 0; i < 100 && detail.run.status === 'running'; i++) {
          await new Promise((r) => setTimeout(r, 300))
          detail = await api.runDetail(outcome.run_id)
        }
        const ok = detail.run.status === 'success'
        setRun(wf.id, { phase: 'done', ok, text: ok ? 'Done! It ran just now.' : `Failed: ${detail.run.error ?? 'unknown error'}` })
      }
    } catch (e) {
      setRun(wf.id, { phase: 'done', ok: false, text: e instanceof Error ? e.message : String(e) })
    }
    load()
    setTimeout(() => setRun(wf.id, null), 5000)
  }

  const [workflows, setWorkflows] = useState<Workflow[] | null>(null)
  const [creating, setCreating] = useState(false)
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const entities = useEntities()
  const fileInput = useRef<HTMLInputElement>(null)

  const exportAll = async () => {
    try {
      downloadExport(await api.exportWorkflows(), 'workflows')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const importFile = async (file: File) => {
    setError(null)
    setNotice(null)
    try {
      let parsed: unknown
      try {
        parsed = JSON.parse(await file.text())
      } catch {
        throw new Error(`${file.name} isn't a JSON file.`)
      }
      const { created } = await api.importWorkflows(parsed)
      setNotice(
        created.length === 1
          ? `Imported “${created[0].name}”. Check its steps, then go live.`
          : `Imported ${created.length} workflows. Check their steps, then go live.`,
      )
      load()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const load = () =>
    api
      .workflows()
      .then(setWorkflows)
      .catch((e) => setError(e.message))

  useEffect(() => {
    load()
    const t = setInterval(load, 8000)
    return () => clearInterval(t)
  }, [])

  const create = async () => {
    if (!name.trim()) return
    const { workflow } = await api.createWorkflow(name.trim())
    window.location.hash = `#/w/${workflow.id}`
  }

  const duplicate = async (wf: Workflow) => {
    setError(null)
    setNotice(null)
    try {
      const { workflow } = await api.duplicateWorkflow(wf.id)
      setNotice(`Made “${workflow.name}”. It's a draft until you go live.`)
      load()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const toggle = async (wf: Workflow) => {
    setWorkflows((list) => list?.map((w) => (w.id === wf.id ? { ...w, enabled: !w.enabled } : w)) ?? null)
    await api.setEnabled(wf.id, !wf.enabled)
    load()
  }

  const newCard = creating ? (
    <form
      className="wf-card new-form"
      onSubmit={(e) => {
        e.preventDefault()
        create()
      }}
    >
      <label className="label" htmlFor="new-name">
        What should it be called?
      </label>
      <input
        id="new-name"
        className="input"
        autoFocus
        placeholder="e.g. Heater on when it's cold"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === 'Escape' && setCreating(false)}
      />
      <div className="new-actions">
        <button type="button" className="btn ghost" onClick={() => setCreating(false)}>
          Cancel
        </button>
        <button className="btn primary" disabled={!name.trim()}>
          Create
        </button>
      </div>
    </form>
  ) : (
    <button className="wf-card new-card" onClick={() => setCreating(true)}>
      <span className="new-plus">
        <Icon name="plus" size={26} />
      </span>
      <span>New workflow</span>
    </button>
  )

  return (
    <>
      <TopBar section="workflows" />
      <main className="page">
        <div className="page-inner">
          <PageHeader
            title="Your workflows"
            subtitle="Something happens at home. Ergo, something gets done."
            actions={
              <>
                <input
                  ref={fileInput}
                  type="file"
                  accept="application/json,.json"
                  hidden
                  onChange={(e) => {
                    const file = e.target.files?.[0]
                    e.target.value = ''
                    if (file) importFile(file)
                  }}
                />
                <button className="btn" onClick={() => fileInput.current?.click()} title="Add workflows from an ergo export file">
                  <Icon name="upload" size={16} /> <span className="hide-narrow">Import</span>
                </button>
                <button className="btn" onClick={exportAll} disabled={!workflows?.length} title="Download every workflow as a file">
                  <Icon name="download" size={16} /> <span className="hide-narrow">Export all</span>
                </button>
              </>
            }
          />

          {error && <div className="issue">{error}</div>}
          {notice && <div className="notice">{notice}</div>}

          {workflows && (
            <div className="wf-grid">
              {newCard}
              {workflows.map((wf) => {
                const { when, then } = describeWorkflow(wf.draft, entities)
                const trigger = wf.draft.nodes.find((n) => n.type.startsWith('trigger.'))
                return (
                  <article key={wf.id} className={`wf-card${wf.enabled && wf.active_version ? '' : ' off'}`}>
                    <a className="wf-link" href={`#/w/${wf.id}`} aria-label={`Open ${wf.name}`} />
                    <div className="wf-top">
                      <span className="wf-icon">
                        <Icon name={trigger ? nodeIcon(trigger.type) : 'sparkle'} size={20} />
                      </span>
                      <h2>{wf.name}</h2>
                      <button
                        className="switch"
                        role="switch"
                        aria-checked={wf.enabled}
                        aria-label={wf.enabled ? 'Turn off' : 'Turn on'}
                        title={wf.enabled ? 'On' : 'Off'}
                        onClick={() => toggle(wf)}
                      />
                    </div>
                    <p className="wf-sentence" title={`When ${when}, then ${then}`}>
                      <span className="faint">When</span> {when}
                      <br />
                      <span className="faint">then</span> {then}
                    </p>
                    {running[wf.id]?.phase === 'running' && <div className="run-banner">Running…</div>}
                    {(() => {
                      const r = running[wf.id]
                      return r?.phase === 'done' ? <div className={`run-banner ${r.ok ? 'ok' : 'err'}`}>{r.text}</div> : null
                    })()}
                    <footer className="wf-foot">
                      {!wf.active_version ? (
                        <span className="pill">Not live yet</span>
                      ) : !wf.enabled ? (
                        <span className="pill">Off</span>
                      ) : wf.dirty ? (
                        <span className="pill amber">Unsaved changes</span>
                      ) : (
                        <span className="pill live">Live</span>
                      )}
                      {wf.manual_trigger && !running[wf.id] && (
                        <button className="btn run-now" onClick={() => setConfirming(wf)}>
                          <Icon name="play" size={15} /> Run now
                        </button>
                      )}
                      <span className="spacer" />
                      <button
                        className="btn ghost icon card-action"
                        onClick={() => duplicate(wf)}
                        aria-label={`Duplicate ${wf.name}`}
                        title="Duplicate workflow"
                      >
                        <Icon name="copy" size={17} />
                      </button>
                      {!(wf.enabled && wf.active_version) && (
                        <button
                          className="btn ghost icon card-action card-delete"
                          onClick={() => setDeleting(wf)}
                          aria-label={`Delete ${wf.name}`}
                          title="Delete workflow"
                        >
                          <Icon name="trash" size={17} />
                        </button>
                      )}
                      {wf.last_run ? (
                        <span className="last-run">
                          <span className={`dot ${wf.last_run.status}`} />
                          {STATUS_TEXT[wf.last_run.status] ?? wf.last_run.status} {ago(wf.last_run.started_at)}
                        </span>
                      ) : (
                        <span className="faint never">Never run</span>
                      )}
                    </footer>
                  </article>
                )
              })}
            </div>
          )}
        </div>
      </main>
      {deleting && (
        <Confirm
          danger
          icon="trash"
          title={<>Delete “{deleting.name}”?</>}
          confirmLabel="Delete workflow"
          onCancel={() => setDeleting(null)}
          onConfirm={async () => {
            const wf = deleting
            setDeleting(null)
            try {
              await api.deleteWorkflow(wf.id)
            } catch (e) {
              setError(e instanceof Error ? e.message : String(e))
            }
            load()
          }}
        >
          This removes the workflow and all of its run history. It can't be undone.
        </Confirm>
      )}
      {confirming && (
        <Confirm
          title={<>Run “{confirming.name}” now?</>}
          confirmLabel="Yes, run it"
          onCancel={() => setConfirming(null)}
          onConfirm={() => {
            const wf = confirming
            setConfirming(null)
            runNow(wf)
          }}
        >
          It will {describeWorkflow(confirming.draft, entities).then} right away, using the live version.
        </Confirm>
      )}
    </>
  )
}
