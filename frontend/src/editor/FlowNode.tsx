import { Handle, type NodeProps, Position } from '@xyflow/react'
import { type CSSProperties, Fragment, memo } from 'react'
import type { RunNode } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { describeNode, nodeIcon } from '../describe.ts'
import { type ErgoNode, PORT_LABEL, useEditor } from './model.ts'

function size(b: number): string {
  if (b < 1024) return `${b} B`
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`
  return `${(b / 1024 / 1024).toFixed(1)} MB`
}

/** 1500 -> "1.5 s", 90000 -> "1 min 30 s". */
function seconds(ms: number): string {
  const s = Math.round(ms / 100) / 10
  if (s < 60) return `${s} s`
  const m = Math.floor(s / 60)
  const rest = Math.round(s % 60)
  return m < 60 ? `${m} min${rest ? ` ${rest} s` : ''}` : `${Math.floor(m / 60)} h ${m % 60} min`
}

function resultText(nodeType: string, r: RunNode): string {
  if (r.error) return r.error.message
  if (nodeType.startsWith('trigger.')) return (r.output as { simulated?: boolean })?.simulated ? 'Started (test)' : 'Started'
  if (nodeType === 'mqtt.publish') return `Sent · ${r.duration_ms} ms`
  if (nodeType === 'flow.if') return r.port === 'true' ? 'Yes, it holds' : 'No, it doesn’t'
  if (nodeType === 'ha.action') return `Done · ${r.duration_ms} ms`
  if (nodeType === 'ha.notify') return `Sent · ${r.duration_ms} ms`
  if (nodeType === 'push.send') {
    const n = ((r.output as { sent?: unknown[] })?.sent ?? []).length
    return `Sent to ${n} browser${n === 1 ? '' : 's'}`
  }
  if (nodeType === 'flow.delay') return `Waited ${seconds(r.duration_ms)}`
  if (nodeType === 'flow.wait') return r.port === 'timeout' ? `Gave up after ${seconds(r.duration_ms)}` : `Reached after ${seconds(r.duration_ms)}`
  const out = (r.output ?? {}) as { status?: number; size?: number; count?: number }
  if (nodeType === 'http.request') return `${out.status} · ${r.duration_ms} ms`
  if (nodeType === 'http.download') return `${out.status} · ${size(out.size ?? 0)} · ${r.duration_ms} ms`
  if (nodeType === 'data.filter') return `Kept ${out.count ?? 0} · ${r.duration_ms} ms`
  return `Done · ${r.duration_ms} ms`
}

/** Where exit `i` of `n` sits, as % of the step's height. */
function portY(i: number, n: number): number {
  return i === 0 ? 50 : 66 + (18 * i) / (n - 1)
}

export const FlowNode = memo(function FlowNode({ id, data, selected }: NodeProps<ErgoNode>) {
  const { schemas, entities, results, runKey, runShown, stale, issues, connected, onAddAfter } = useEditor()
  const schema = schemas.get(data.nodeType)
  const kind = schema?.kind ?? 'action'
  const edited = stale.has(id)
  const result = edited ? undefined : results.get(id)
  const hasError = (issues.get(id) ?? []).some((i) => i.severity === 'error')
  const ports = schema?.ports ?? ['out']
  const text = describeNode(data.nodeType, data.config, entities)
  const incomplete = /^(Pick|Choose)/.test(text)

  return (
    <div
      className={`step kind-${kind}${selected ? ' selected' : ''}${
        (hasError && !incomplete) || result?.error ? ' has-error' : ''
      }${runShown && !result && !edited ? ' not-reached' : ''}${
        incomplete || hasError ? ' incomplete' : ''
      }`}
    >
      {kind !== 'trigger' && <Handle type="target" position={Position.Left} />}
      <span className="step-icon">
        <Icon name={nodeIcon(data.nodeType)} size={22} />
      </span>
      <div className="step-body">
        <div className="step-kicker">
          {kind === 'trigger' ? 'When' : 'Then'} · {data.label || schema?.title || data.nodeType}
        </div>
        <div className="step-text">{text}</div>
      </div>
      {ports.length === 1 ? (
        <Handle type="source" id={ports[0]} position={Position.Right} />
      ) : (
        // Several exits (e.g. out / empty): the main one stays in the middle so
        // the main line is straight; the others sit lower down, named.
        ports.map((port, i) => {
          const y = { '--port-y': `${portY(i, ports.length)}%` } as CSSProperties
          return (
            <Fragment key={port}>
              <Handle type="source" id={port} position={Position.Right} className="port-multi" style={y} />
              <span className={`port-label${i ? ' side' : ''}`} style={y}>
                {PORT_LABEL[port] ?? port}
              </span>
            </Fragment>
          )
        })
      )}
      {/* Each exit with nothing after it gets a + (on a connected exit it
          would sit on the connection line). */}
      {ports.map((port, i) =>
        connected.has(`${id}:${port}`) ? null : (
          <button
            key={port}
            className={`step-add nodrag${ports.length > 1 ? ' on-port' : ''}`}
            style={ports.length > 1 ? ({ '--port-y': `${portY(i, ports.length)}%` } as CSSProperties) : undefined}
            title={ports.length > 1 ? `Add a step for “${PORT_LABEL[port] ?? port}”` : 'Add a step after this'}
            aria-label={ports.length > 1 ? `Add a step for ${PORT_LABEL[port] ?? port}` : 'Add a step after this'}
            onClick={(e) => {
              e.stopPropagation()
              onAddAfter(id, port)
            }}
          >
            <Icon name="plus" size={ports.length > 1 ? 14 : 16} />
          </button>
        ),
      )}
      {result && (
        <div key={runKey} className={`step-result ${result.error ? 'err' : 'ok'}`}>
          <Icon name={result.error ? 'x' : 'check'} size={14} />
          <span>{resultText(data.nodeType, result)}</span>
        </div>
      )}
      {edited && (
        <div key={runKey} className="step-result skipped">
          <span>Edited since this run</span>
        </div>
      )}
      {!result && !edited && runShown && (
        <div key={runKey} className="step-result skipped">
          <span>{kind === 'trigger' ? 'Not this run’s trigger' : 'Not reached in this run'}</span>
        </div>
      )}
    </div>
  )
})
