import { useEffect, useRef, useState } from 'react'
import { api, type Entity, type NodeSchema, type Ready } from './api.ts'

/** Polls /ready every 5 s for the status bar. */
export function useReady(): Ready | null {
  const [ready, setReady] = useState<Ready | null>(null)
  useEffect(() => {
    let alive = true
    const load = () =>
      api
        .ready()
        .then((r) => alive && setReady(r))
        .catch(() => alive && setReady(null))
    load()
    const t = setInterval(load, 5000)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [])
  return ready
}

let schemasCache: Promise<NodeSchema[]> | null = null
export function useSchemas(): Map<string, NodeSchema> {
  const [schemas, setSchemas] = useState<Map<string, NodeSchema>>(new Map())
  useEffect(() => {
    schemasCache ??= api.nodes()
    schemasCache.then((list) => setSchemas(new Map(list.map((s) => [s.type, s]))))
  }, [])
  return schemas
}

/** Live HA entities, refreshed every 10 s while mounted. */
export function useEntities(): Entity[] {
  const [entities, setEntities] = useState<Entity[]>([])
  useEffect(() => {
    let alive = true
    const load = () => api.entities().then((e) => alive && setEntities(e)).catch(() => {})
    load()
    const t = setInterval(load, 10000)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [])
  return entities
}

/** Calls `fn` `ms` after the last change to `deps`. */
export function useDebounced(fn: () => void, deps: unknown[], ms: number) {
  const fnRef = useRef(fn)
  fnRef.current = fn
  useEffect(() => {
    const t = setTimeout(() => fnRef.current(), ms)
    return () => clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
}
