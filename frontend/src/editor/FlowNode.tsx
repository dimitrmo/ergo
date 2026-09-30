import { Handle, type NodeProps, Position } from '@xyflow/react'
import { type CSSProperties, Fragment, memo } from 'react'
import type { RunNode } from '../api.ts'
import { Icon } from '../components/Icon.tsx'
import { describeNode, nodeIcon } from '../describe.ts'
import { type ErgoNode, useEditor } from './model.ts'

function size(b: number): string {
  if (b < 1024) return `${b} B`
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`
  return `${(b / 1024 / 1024).toFixed(1)} MB`
}

function resultText(nodeType: string, r: RunNode): string {
  if (r.error) return r.error.message
  if (nodeType.startsWith('trigger.')) return (r.output as { simulated?: boolean })?.simulated ? 'Started (test)' : 'Started'
  if (nodeType === 'mqtt.publish') return `Sent · ${r.duration_ms} ms`
  const out = (r.output ?? {}) as { status?: number; size?: number; count?: number }
  if (nodeType === 'http.request') return `${out.status} · ${r.duration_ms} ms`
  if (nodeType === 'http.download') return `${out.status} · ${size(out.size ?? 0)} · ${r.duration_ms} ms`
  if (nodeType === 'data.filter') return `Kept ${out.count ?? 0} · ${r.duration_ms} ms`
  return `Done · ${r.duration_ms} ms`
}

export const FlowNode = memo(function FlowNode({ id, data, selected }: NodeProps<ErgoNode>) {
  const { schemas, entities, results, runKey, runShown, issues, connected, onAddAfter } = useEditor()
  const schema = schemas.get(data.nodeType)
  const kind = schema?.kind ?? 'action'
  const result = results.get(id)
  const hasError = (issues.get(id) ?? []).some((i) => i.severity === 'error')
  const ports = schema?.ports ?? ['out']
  const text = describeNode(data.nodeType, data.config, entities)
  const incomplete = /^(Pick|Choose)/.test(text)

  return (
    <div
      className={`step kind-${kind}${selected ? ' selected' : ''}${
        (hasError && !incomplete) || result?.error ? ' has-error' : ''
      }${runShown && !result ? ' not-reached' : ''}${
        incomplete || hasError ? ' incomplete' : ''
      }`}
    >
      {kind !== 'trigger' && <Handle type="target" position={Position.Left} />}
      <span className="step-icon">
        <Icon name={nodeIcon(data.nodeType)} size={22} />
      </span>
      <div className="step-body">
        <div className="step-kicker">
          {kind === 'trigger' ? 'When' : 'Then'} · {data.label || schema?.title}
        </div>
        <div className="step-text">{text}</div>
      </div>
      {ports.length === 1 ? (
        <Handle type="source" id={ports[0]} position={Position.Right} />
      ) : (
        // Several exits (e.g. out / empty): the main one stays in the middle so
        // the main line is straight; the others sit lower down, named.
        ports.map((port, i) => {
          const pct = i === 0 ? 50 : 66 + (18 * i) / (ports.length - 1)
          const y = { '--port-y': `${pct}%` } as CSSProperties
          return (
            <Fragment key={port}>
              <Handle type="source" id={port} position={Position.Right} className="port-multi" style={y} />
              <span className={`port-label${i ? ' side' : ''}`} style={y}>
                {port}
              </span>
            </Fragment>
          )
        })
      )}
      {/* Only the end of a chain gets a + (on a connected step it would sit
          on the connection line); branching uses the dot or the side panel. */}
      {!connected.has(id) && (
        <button
          className="step-add nodrag"
          title="Add a step after this"
          aria-label="Add a step after this"
          onClick={(e) => {
            e.stopPropagation()
            onAddAfter(id)
          }}
        >
          <Icon name="plus" size={16} />
        </button>
      )}
      {result && (
        <div key={runKey} className={`step-result ${result.error ? 'err' : 'ok'}`}>
          <Icon name={result.error ? 'x' : 'check'} size={14} />
          <span>{resultText(data.nodeType, result)}</span>
        </div>
      )}
      {!result && runShown && (
        <div key={runKey} className="step-result skipped">
          <span>{kind === 'trigger' ? 'Not this run’s trigger' : 'Not reached in this run'}</span>
        </div>
      )}
    </div>
  )
})
