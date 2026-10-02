import {
  addEdge,
  Background,
  BackgroundVariant,
  type Connection,
  Controls,
  type Edge,
  type IsValidConnection,
  MarkerType,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesInitialized,
  useNodesState,
  useReactFlow,
} from '@xyflow/react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ApiError, api, downloadExport, type Issue, type Run, type RunDetail, type RunNode, type Workflow } from '../api.ts'
import { Confirm } from '../components/Confirm.tsx'
import { Icon } from '../components/Icon.tsx'
import { TopBar } from '../components/TopBar.tsx'
import { AddStep } from '../editor/AddStep.tsx'
import { FlowNode } from '../editor/FlowNode.tsx'
import {
  defaults,
  EditorContext,
  edgeId,
  type ErgoNode,
  freeSpot,
  GAP_X,
  NODE_H,
  NODE_W,
  nextId,
  toFlow,
  toGraph,
} from '../editor/model.ts'
import { History } from '../editor/RunsPanel.tsx'
import { StepPanel } from '../editor/StepPanel.tsx'
import { useDebounced, useEntities, useSchemas } from '../hooks.ts'
import '../editor/Editor.css'

const nodeTypes = { ergo: FlowNode }
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

type Toast = { kind: 'ok' | 'error'; title: string; text?: string } | null
/** How close (in px) a dropped step must be to a neighbour's height to snap into line. */
const ALIGN_SNAP = 24

type Chooser = { mode: 'trigger' } | { mode: 'step'; after: string } | null

function successText(detail: RunDetail): string {
  const last = [...detail.nodes].reverse().find((n) => n.node_type === 'mqtt.publish')
  const out = last?.output as { topic?: string; payload?: string } | undefined
  if (out?.topic) return `Sent “${out.payload || '(empty)'}” to ${out.topic}.`
  return `Every step finished in ${detail.nodes.reduce((s, n) => s + n.duration_ms, 0)} ms.`
}

export function Editor({ id }: { id: string }) {
  return (
    <ReactFlowProvider>
      <EditorInner id={id} />
    </ReactFlowProvider>
  )
}

function EditorInner({ id }: { id: string }) {
  const schemas = useSchemas()
  const entities = useEntities()
  const { fitView } = useReactFlow()

  const [workflow, setWorkflow] = useState<Workflow | null>(null)
  const [missing, setMissing] = useState(false)
  const [issues, setIssues] = useState<Issue[]>([])
  const [name, setName] = useState('')
  const [nodes, setNodes, onNodesChange] = useNodesState<ErgoNode>([])
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [chooser, setChooser] = useState<Chooser>(null)
  const [saveState, setSaveState] = useState<'saved' | 'saving' | 'error'>('saved')
  const [runs, setRuns] = useState<Run[]>([])
  const [shown, setShown] = useState<RunDetail | null>(null)
  const [follow, setFollow] = useState(true)
  const [busy, setBusy] = useState<'live' | 'run' | null>(null)
  const [toast, setToast] = useState<Toast>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)

  const savedSnapshot = useRef('')
  const loaded = useRef(false)
  const fitted = useRef(false)
  // The canvas stays hidden behind a loader until the first fit is done, so
  // the nodes never visibly jump into place.
  const [ready, setReady] = useState(false)

  const graph = useMemo(() => toGraph(nodes, edges), [nodes, edges])
  const snapshot = useMemo(() => JSON.stringify({ name, graph }), [name, graph])

  const showRun = useCallback(async (runId: string) => setShown(await api.runDetail(runId)), [])
  const loadRuns = useCallback(async () => {
    const list = await api.runs(id, 40)
    setRuns(list)
    return list
  }, [id])

  const measured = useNodesInitialized()

  useEffect(() => {
    api
      .workflow(id)
      .then(async ({ workflow, issues }) => {
        const flow = toFlow(workflow.draft)
        setWorkflow(workflow)
        setIssues(issues)
        setName(workflow.name)
        setNodes(flow.nodes)
        setEdges(flow.edges)
        savedSnapshot.current = JSON.stringify({ name: workflow.name, graph: toGraph(flow.nodes, flow.edges) })
        loaded.current = true
        if (!flow.nodes.length) setReady(true)
        const list = await loadRuns()
        if (list[0]) showRun(list[0].id)
      })
      .catch((e) => (e instanceof ApiError && e.status === 404 ? setMissing(true) : setToast({ kind: 'error', title: e.message })))
  }, [id, loadRuns, showRun, setNodes, setEdges])

  // Runs from real triggers appear without a reload.
  useEffect(() => {
    const t = setInterval(async () => {
      const list = await loadRuns().catch(() => null)
      if (list?.[0] && follow && busy !== 'run' && list[0].id !== shown?.run.id && list[0].status !== 'running') showRun(list[0].id)
    }, 4000)
    return () => clearInterval(t)
  }, [loadRuns, showRun, follow, shown, busy])

  // Keep the workflow in view: fit once the nodes are measured, again when a
  // step is added, and whenever the canvas changes size (a panel opening or
  // closing, the window resizing). With a step selected, zoom in on that step
  // instead; closing its panel zooms back out to the whole workflow.
  const canvasRef = useRef<HTMLDivElement>(null)
  const focusRef = useRef<string | null>(null)
  useEffect(() => {
    focusRef.current = selected
  }, [selected])
  const fit = useCallback(() => {
    const first = !fitted.current
    const focus = focusRef.current
    const view = focus
      ? { nodes: [{ id: focus }], duration: first ? 0 : 300, padding: 0.6, minZoom: 1, maxZoom: 1.2 }
      : { duration: first ? 0 : 250, padding: 0.35, maxZoom: 0.9 }
    fitView(view).then(() => {
      // Reveal on the next frame, once the fitted view has been painted.
      if (first) requestAnimationFrame(() => setReady(true))
    })
    fitted.current = true
  }, [fitView])

  useEffect(() => {
    // Only the first fit waits for React Flow to measure the nodes. Its
    // `measured` flag drops whenever node objects are replaced (e.g. on
    // selection), so later fits don't depend on it.
    if (!fitted.current && !measured) return
    // One tick later, so React Flow has stored the measured sizes.
    const t = setTimeout(fit, 0)
    return () => clearTimeout(t)
  }, [measured, nodes.length, fit])

  // Selecting a step, or opening or closing a panel (which changes the canvas
  // width): refit once the new layout has settled. The resize observer below
  // also covers window resizes.
  useEffect(() => {
    if (!fitted.current) return
    const t = setTimeout(fit, 120)
    return () => clearTimeout(t)
  }, [selected, historyOpen, fit])

  useEffect(() => {
    const el = canvasRef.current
    if (!el) return
    let width = el.clientWidth
    let timer: ReturnType<typeof setTimeout> | undefined
    const observer = new ResizeObserver(() => {
      if (el.clientWidth === width) return
      width = el.clientWidth
      clearTimeout(timer)
      // React Flow sees the new size through its own observer; fit just after.
      timer = setTimeout(() => fitted.current && fit(), 50)
    })
    observer.observe(el)
    return () => {
      observer.disconnect()
      clearTimeout(timer)
    }
  }, [fit])

  useEffect(() => {
    if (!toast) return
    const t = setTimeout(() => setToast(null), 5000)
    return () => clearTimeout(t)
  }, [toast])

  const save = useCallback(async () => {
    if (!loaded.current || snapshot === savedSnapshot.current) return
    const snap = snapshot
    setSaveState('saving')
    try {
      const r = await api.saveWorkflow(id, { name: name.trim() || undefined, draft: graph })
      savedSnapshot.current = snap
      setWorkflow(r.workflow)
      setIssues(r.issues)
      setSaveState('saved')
    } catch {
      setSaveState('error')
    }
  }, [id, name, graph, snapshot])

  useDebounced(save, [snapshot], 500)

  const goLive = async () => {
    setBusy('live')
    try {
      await save()
      const r = await api.activate(id)
      setWorkflow(r.workflow)
      setIssues(r.issues)
      setToast({ kind: 'ok', title: "It's live!", text: 'ergo will run this workflow whenever its trigger fires.' })
    } catch (e) {
      if (e instanceof ApiError && e.issues.length) setIssues(e.issues)
      setToast({ kind: 'error', title: 'Not quite ready', text: e instanceof Error ? e.message : String(e) })
    } finally {
      setBusy(null)
    }
  }

  const tryIt = useCallback(async () => {
    if (!nodes.length) return
    setBusy('run')
    setShown(null)
    try {
      await save()
      const outcome = await api.run(id, { draft: true })
      if (outcome.outcome === 'skipped') {
        setToast({ kind: 'error', title: 'Skipped', text: outcome.reason })
        return
      }
      setFollow(true)
      for (let i = 0; i < 150; i++) {
        await sleep(i === 0 ? 350 : 300)
        const detail = await api.runDetail(outcome.run_id)
        if (detail.run.status !== 'running') {
          setShown(detail)
          setToast(
            detail.run.status === 'success'
              ? { kind: 'ok', title: 'It worked!', text: successText(detail) }
              : { kind: 'error', title: "That didn't work", text: detail.run.error ?? undefined },
          )
          break
        }
      }
      loadRuns()
    } catch (e) {
      // Nothing ran: don't let the run poll bring back an older run as if it were this one.
      setFollow(false)
      if (e instanceof ApiError && e.issues.length) setIssues(e.issues)
      setToast({ kind: 'error', title: 'Almost there', text: e instanceof Error ? e.message : String(e) })
    } finally {
      setBusy(null)
    }
  }, [id, nodes.length, save, loadRuns])

  const deleteWorkflow = async () => {
    setConfirmDelete(false)
    try {
      await api.deleteWorkflow(id)
      window.location.hash = '#/'
    } catch (e) {
      setToast({ kind: 'error', title: "Couldn't delete it", text: e instanceof Error ? e.message : String(e) })
    }
  }

  const exportThis = async () => {
    if (!workflow) return
    try {
      await save()
      downloadExport(await api.exportWorkflows([id]), workflow.name)
    } catch (e) {
      setToast({ kind: 'error', title: "Couldn't export it", text: e instanceof Error ? e.message : String(e) })
    }
  }

  const duplicateThis = async () => {
    try {
      await save()
      const { workflow: copy } = await api.duplicateWorkflow(id)
      window.location.hash = `#/w/${copy.id}`
    } catch (e) {
      setToast({ kind: 'error', title: "Couldn't duplicate it", text: e instanceof Error ? e.message : String(e) })
    }
  }

  const toggleEnabled = async () => {
    if (!workflow) return
    const r = await api.setEnabled(id, !workflow.enabled)
    setWorkflow(r.workflow)
  }

  const select = (nodeId: string | null) => {
    setNodes((ns) => ns.map((n) => ({ ...n, selected: n.id === nodeId })))
    setSelected(nodeId)
    if (nodeId) setHistoryOpen(false)
  }

  // Steps all have the same height, so equal y means a straight connection.
  // A step dropped close to a connected step's height snaps into line with it.
  const alignWithNeighbour = (nodeId: string) => {
    setNodes((ns) => {
      const node = ns.find((n) => n.id === nodeId)
      if (!node) return ns
      const neighbours = new Set(
        edges.flatMap((e) => (e.source === nodeId ? [e.target] : e.target === nodeId ? [e.source] : [])),
      )
      let best: number | null = null
      for (const n of ns) {
        if (!neighbours.has(n.id)) continue
        const dy = Math.abs(n.position.y - node.position.y)
        if (dy > 0 && dy <= ALIGN_SNAP && (best === null || dy < Math.abs(best - node.position.y))) best = n.position.y
      }
      if (best === null) return ns
      const y = best
      return ns.map((n) => (n.id === nodeId ? { ...n, position: { ...n.position, y } } : n))
    })
  }

  const addNode = (type: string, how: NonNullable<Chooser>) => {
    const schema = schemas.get(type)
    if (!schema) return
    const chooser = how
    const nid = nextId(nodes)
    let position: { x: number; y: number }
    if (chooser.mode === 'step') {
      const from = nodes.find((n) => n.id === chooser.after)!
      position = freeSpot(nodes, from.position.x + NODE_W + GAP_X, from.position.y)
    } else if (nodes.length) {
      const left = Math.min(...nodes.map((n) => n.position.x))
      position = freeSpot(nodes, left, Math.min(...nodes.map((n) => n.position.y)) + NODE_H)
    } else {
      position = { x: 0, y: 0 }
    }
    const node: ErgoNode = { id: nid, type: 'ergo', position, selected: true, data: { nodeType: type, config: defaults(schema) } }
    setNodes((ns) => [...ns.map((n) => ({ ...n, selected: false })), node])
    if (chooser.mode === 'step') {
      const port = schemas.get(nodes.find((n) => n.id === chooser.after)!.data.nodeType)?.ports[0] ?? 'out'
      const c = { source: chooser.after, sourceHandle: port, target: nid }
      setEdges((es) => [...es, { ...c, id: edgeId(c) }])
    } else {
      // "Another trigger" means "or start the same steps when…": connect it
      // to wherever the existing triggers lead.
      const triggerIds = new Set(nodes.filter((n) => schemas.get(n.data.nodeType)?.kind === 'trigger').map((n) => n.id))
      const targets = [...new Set(edges.filter((e) => triggerIds.has(e.source)).map((e) => e.target))]
      setEdges((es) => [
        ...es,
        ...targets.map((target) => {
          const c = { source: nid, sourceHandle: 'out', target }
          return { ...c, id: edgeId(c) }
        }),
      ])
    }
    setSelected(nid)
    setHistoryOpen(false)
    setChooser(null)
  }

  // "Add step": continue from the selected step, or from the last step that
  // has nothing after it (the rightmost end of the flow).
  const addStep = () => {
    const from =
      nodes.find((n) => n.id === selected) ??
      [...nodes]
        .filter((n) => !edges.some((e) => e.source === n.id))
        .sort((a, b) => b.position.x - a.position.x || a.position.y - b.position.y)[0] ??
      nodes[nodes.length - 1]
    if (from) setChooser({ mode: 'step', after: from.id })
  }

  const updateNode = (nodeId: string, patch: Partial<ErgoNode['data']>) =>
    setNodes((ns) => ns.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, ...patch } } : n)))

  const deleteNode = (nodeId: string) => {
    setNodes((ns) => ns.filter((n) => n.id !== nodeId))
    setEdges((es) => es.filter((e) => e.source !== nodeId && e.target !== nodeId))
    setSelected(null)
  }

  const onConnect = useCallback((c: Connection) => setEdges((es) => addEdge({ ...c, id: edgeId(c) }, es)), [setEdges])

  const isValidConnection: IsValidConnection = useCallback(
    (c) => {
      if (c.source === c.target) return false
      const target = nodes.find((n) => n.id === c.target)
      if (target && schemas.get(target.data.nodeType)?.kind === 'trigger') return false
      return !edges.some((e) => e.id === edgeId(c))
    },
    [nodes, edges, schemas],
  )

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault()
        tryIt()
      } else if (e.key === 'Escape' && !chooser) {
        setSelected(null)
        setHistoryOpen(false)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [tryIt, chooser])

  const issuesByNode = useMemo(() => {
    const m = new Map<string, Issue[]>()
    for (const i of issues) if (i.node) m.set(i.node, [...(m.get(i.node) ?? []), i])
    return m
  }, [issues])

  const results = useMemo(() => {
    const m = new Map<string, RunNode>()
    for (const n of shown?.nodes ?? []) m.set(n.node_id, n)
    return m
  }, [shown])

  // Each step's config when the shown run came on screen. A step edited since
  // then keeps its old result in the inspector but stops showing it as current.
  const [runConfigs, setRunConfigs] = useState(new Map<string, string>())
  const [configsFor, setConfigsFor] = useState<string | null>(null)
  if (shown && shown.run.id !== configsFor) {
    setConfigsFor(shown.run.id)
    setRunConfigs(new Map(nodes.map((n) => [n.id, JSON.stringify(n.data.config)])))
  }
  const stale = useMemo(() => {
    const s = new Set<string>()
    for (const n of nodes) {
      const was = runConfigs.get(n.id)
      if (results.has(n.id) && was !== undefined && was !== JSON.stringify(n.data.config)) s.add(n.id)
    }
    return s
  }, [nodes, runConfigs, results])

  const connected = useMemo(() => new Set(edges.map((e) => e.source)), [edges])
  const context = useMemo(
    () => ({
      schemas,
      entities,
      results,
      runKey: shown?.run.id ?? '',
      runShown: !!shown && shown.run.status !== 'running',
      stale,
      issues: issuesByNode,
      connected,
      onAddAfter: (nodeId: string) => setChooser({ mode: 'step', after: nodeId }),
    }),
    [schemas, entities, results, shown, stale, issuesByNode, connected],
  )

  const running = busy === 'run'
  const shownEdges = useMemo(
    () =>
      edges.map((e) => ({
        ...e,
        type: 'smoothstep',
        animated: running,
        className: running ? 'flowing' : undefined,
        markerEnd: { type: MarkerType.ArrowClosed, width: 18, height: 18, color: running ? '#5e5ce6' : 'rgba(235,235,245,0.5)' },
      })),
    [edges, running],
  )

  const selectedNode = nodes.find((n) => n.id === selected)
  // The selected step's input comes from the steps connected into it.
  const upstream = useMemo(() => {
    if (!selected) return []
    const sources = new Set(edges.filter((e) => e.target === selected).map((e) => e.source))
    return nodes
      .filter((n) => sources.has(n.id))
      .map((n) => schemas.get(n.data.nodeType))
      .filter((s): s is NonNullable<typeof s> => !!s)
  }, [selected, edges, nodes, schemas])
  const problems = issues.filter((i) => i.severity === 'error')
  const drawer = selectedNode ? 'step' : historyOpen ? 'history' : null

  if (missing) {
    return (
      <>
        <TopBar />
        <main className="page">
          <div className="page-inner">
            <h1>This workflow is gone</h1>
            <p className="quiet">It may have been deleted.</p>
            <a className="btn" href="#/">
              Back to your workflows
            </a>
          </div>
        </main>
      </>
    )
  }

  const status = !workflow
    ? null
    : !workflow.active_version
      ? { cls: '', text: 'Not live yet' }
      : !workflow.enabled
        ? { cls: '', text: 'Off' }
        : workflow.dirty
          ? { cls: 'amber', text: 'Changes not live' }
          : { cls: 'live', text: 'Live' }

  return (
    <>
      <TopBar
        left={
          <div className="editor-title">
            <a className="btn ghost icon" href="#/" aria-label="Back to workflows" title="Back to workflows">
              <Icon name="back" />
            </a>
            <input
              className="name-input"
              size={Math.max(name.length, 8)}
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-label="Workflow name"
              spellCheck={false}
            />
            {status && <span className={`pill ${status.cls}`}>{status.text}</span>}
            {saveState !== 'saved' && (
              <span className="faint save-state">{saveState === 'saving' ? 'Saving…' : "Couldn't save"}</span>
            )}
          </div>
        }
      >
        {workflow?.active_version && (
          <label className="onoff hide-narrow" title="Turn the live workflow on or off">
            <button className="switch" role="switch" aria-checked={workflow.enabled} onClick={toggleEnabled} />
            <span>{workflow.enabled ? 'On' : 'Off'}</span>
          </label>
        )}
        {workflow && (
          <button className="btn ghost icon" onClick={duplicateThis} aria-label="Duplicate workflow" title="Make a copy of this workflow">
            <Icon name="copy" size={18} />
          </button>
        )}
        {workflow && (
          <button className="btn ghost icon" onClick={exportThis} aria-label="Export workflow" title="Download this workflow as a file">
            <Icon name="download" size={18} />
          </button>
        )}
        {workflow && (
          <button
            className="btn ghost icon"
            onClick={() => setConfirmDelete(true)}
            disabled={workflow.enabled && !!workflow.active_version}
            aria-label="Delete workflow"
            title={workflow.enabled && workflow.active_version ? 'Turn it off first to delete it' : 'Delete workflow'}
          >
            <Icon name="trash" size={18} />
          </button>
        )}
        <button
          className={`btn${historyOpen ? ' active' : ''}`}
          onClick={() => {
            setSelected(null)
            setHistoryOpen(!historyOpen)
          }}
        >
          <Icon name="history" size={17} /> <span className="hide-narrow">History</span>
        </button>
        <button className="btn" onClick={tryIt} disabled={busy !== null || !nodes.length} title="Ctrl+Enter">
          <Icon name="play" size={16} /> {running ? 'Running…' : 'Try it'}
        </button>
        <button
          className="btn primary"
          onClick={goLive}
          disabled={busy !== null || !workflow || !nodes.length || problems.length > 0 || (!workflow.dirty && !!workflow.active_version)}
          title={problems.length ? problems.map((p) => p.message).join('\n') : undefined}
        >
          {busy === 'live' ? 'Going live…' : workflow?.active_version ? 'Update live' : 'Go live'}
        </button>
      </TopBar>

      <EditorContext.Provider value={context}>
        <main className={`editor${drawer ? ' with-drawer' : ''}`}>
          <div className={`canvas${ready ? ' ready' : ''}`} ref={canvasRef}>
            <ReactFlow
              nodes={nodes}
              edges={shownEdges}
              nodeTypes={nodeTypes}
              onNodesChange={onNodesChange}
              onEdgesChange={onEdgesChange}
              onConnect={onConnect}
              connectOnClick={false}
              colorMode="dark"
              isValidConnection={isValidConnection}
              onNodeClick={(_, n) => select(n.id)}
              onPaneClick={() => select(null)}
              deleteKeyCode={['Delete', 'Backspace']}
              onNodesDelete={() => setSelected(null)}
              onNodeDragStop={(_, dragged) => alignWithNeighbour(dragged.id)}
              snapToGrid
              snapGrid={[10, 10]}
              minZoom={0.3}
              maxZoom={1.6}
            >
              <Background variant={BackgroundVariant.Dots} gap={22} size={1.4} color="#222" />
              <Controls showInteractive={false} position="bottom-left" />
            </ReactFlow>

            {!ready && (
              <div className="canvas-loading" role="status">
                <span className="spinner" />
                <span>Loading workflow…</span>
              </div>
            )}

            {ready && nodes.length > 0 && (
              <div className="canvas-tools">
                <button className="btn" onClick={addStep}>
                  <Icon name="plus" size={16} /> Add step
                </button>
                <button className="btn" onClick={() => setChooser({ mode: 'trigger' })}>
                  <Icon name="plus" size={16} /> Another trigger
                </button>
                {problems.length > 0 && (
                  <span className="todo">
                    {problems.length === 1 ? '1 thing to finish' : `${problems.length} things to finish`}
                  </span>
                )}
              </div>
            )}

            {schemas.size > 0 && workflow && !nodes.length && (
              <AddStep mode="trigger" schemas={schemas} onPick={(t) => addNode(t, { mode: 'trigger' })} />
            )}

            {toast && (
              <div className={`toast ${toast.kind}`} role="status">
                <span className="toast-icon">
                  <Icon name={toast.kind === 'ok' ? 'check' : 'x'} size={18} />
                </span>
                <div>
                  <strong>{toast.title}</strong>
                  {toast.text && <div className="toast-text">{toast.text}</div>}
                </div>
              </div>
            )}
          </div>

          {drawer === 'step' && selectedNode && (
            <StepPanel
              key={selectedNode.id}
              node={selectedNode}
              result={results.get(selectedNode.id)}
              upstream={upstream}
              onChange={(p) => updateNode(selectedNode.id, p)}
              onDelete={() => deleteNode(selectedNode.id)}
              onClose={() => select(null)}
            />
          )}
          {drawer === 'history' && (
            <History
              runs={runs}
              shownRunId={shown?.run.id ?? null}
              onShow={(runId) => {
                setFollow(runId === runs[0]?.id)
                showRun(runId)
              }}
              onClose={() => setHistoryOpen(false)}
            />
          )}
        </main>

        {confirmDelete && workflow && (
          <Confirm
            danger
            icon="trash"
            title={<>Delete “{name || workflow.name}”?</>}
            confirmLabel="Delete workflow"
            onCancel={() => setConfirmDelete(false)}
            onConfirm={deleteWorkflow}
          >
            This removes the workflow and all of its run history. It can't be undone.
          </Confirm>
        )}
        {chooser && (
          <AddStep mode={chooser.mode} schemas={schemas} onPick={(t) => addNode(t, chooser)} onClose={() => setChooser(null)} />
        )}
      </EditorContext.Provider>
    </>
  )
}
