import type { Edge, Node } from '@xyflow/react'
import { createContext, useContext } from 'react'
import type { Entity, Graph, Issue, NodeSchema, RunNode } from '../api.ts'

export type ErgoNodeData = {
  nodeType: string
  label?: string
  config: Record<string, unknown>
}

export type ErgoNode = Node<ErgoNodeData, 'ergo'>

/** Shared with custom node components without threading props through React Flow. */
export interface EditorContextValue {
  schemas: Map<string, NodeSchema>
  entities: Entity[]
  /** Results of the run being shown on the canvas, by node id. */
  results: Map<string, RunNode>
  /** Changes whenever a new run is shown, so result badges can animate in. */
  runKey: string
  /** A finished run is shown: steps without a record weren't reached. */
  runShown: boolean
  issues: Map<string, Issue[]>
  /** Nodes with at least one outgoing connection. */
  connected: Set<string>
  onAddAfter: (nodeId: string) => void
}

export const EditorContext = createContext<EditorContextValue>({
  schemas: new Map(),
  entities: [],
  results: new Map(),
  runKey: '',
  runShown: false,
  issues: new Map(),
  connected: new Set(),
  onAddAfter: () => {},
})

export const useEditor = () => useContext(EditorContext)

export const edgeId = (c: { source: string; sourceHandle?: string | null; target: string }) =>
  `${c.source}:${c.sourceHandle ?? 'out'}->${c.target}`

export function toFlow(graph: Graph): { nodes: ErgoNode[]; edges: Edge[] } {
  return {
    nodes: graph.nodes.map((n) => ({
      id: n.id,
      type: 'ergo',
      position: n.position,
      data: { nodeType: n.type, label: n.label, config: n.config ?? {} },
    })),
    edges: graph.edges.map((e) => ({
      id: edgeId({ source: e.from, sourceHandle: e.fromPort, target: e.to }),
      source: e.from,
      sourceHandle: e.fromPort,
      target: e.to,
    })),
  }
}

export function toGraph(nodes: ErgoNode[], edges: Edge[]): Graph {
  return {
    schema_version: 1,
    mode: 'single',
    nodes: nodes.map((n) => ({
      id: n.id,
      type: n.data.nodeType,
      ...(n.data.label ? { label: n.data.label } : {}),
      config: n.data.config,
      position: { x: Math.round(n.position.x), y: Math.round(n.position.y) },
    })),
    edges: edges.map((e) => ({ from: e.source, fromPort: e.sourceHandle ?? 'out', to: e.target })),
  }
}

export function nextId(nodes: ErgoNode[]): string {
  const max = nodes.reduce((m, n) => Math.max(m, Number(n.id.replace(/^n/, '')) || 0), 0)
  return `n${max + 1}`
}

export function defaults(schema: NodeSchema): Record<string, unknown> {
  const config: Record<string, unknown> = {}
  for (const f of schema.fields) if (f.default !== undefined) config[f.key] = f.default
  return config
}

export const NODE_W = 250
export const NODE_H = 78
export const GAP_X = 120
export const GAP_Y = 44

/** First spot at `x` from `y` downwards that doesn't overlap another node. */
export function freeSpot(nodes: ErgoNode[], x: number, y: number): { x: number; y: number } {
  let yy = y
  const hit = (py: number) =>
    nodes.some((n) => Math.abs(n.position.x - x) < NODE_W && Math.abs(n.position.y - py) < NODE_H + GAP_Y / 2)
  while (hit(yy)) yy += NODE_H + GAP_Y
  return { x, y: yy }
}
