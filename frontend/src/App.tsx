import { useEffect, useState } from 'react'
import { StatusBar } from './components/StatusBar.tsx'
import { ReadyContext, useReady } from './hooks.ts'
import { Database } from './pages/Database.tsx'
import { Editor } from './pages/Editor.tsx'
import { Mqtt } from './pages/Mqtt.tsx'
import { Status } from './pages/Status.tsx'
import { Workflows } from './pages/Workflows.tsx'

// Hash routing keeps every URL under the Ingress path prefix.
function useHash() {
  const [hash, setHash] = useState(window.location.hash)
  useEffect(() => {
    const on = () => setHash(window.location.hash)
    window.addEventListener('hashchange', on)
    return () => window.removeEventListener('hashchange', on)
  }, [])
  return hash.replace(/^#/, '') || '/'
}

export default function App() {
  const route = useHash()
  const ready = useReady()
  const editing = route.match(/^\/w\/([\w-]+)$/)
  const mqtt = ready?.checks.mqtt.enabled

  return (
    <ReadyContext.Provider value={ready}>
      <div className="shell">
        {editing ? (
          <Editor key={editing[1]} id={editing[1]} />
        ) : route === '/data' ? (
          <Database />
        ) : route === '/mqtt' && mqtt ? (
          <Mqtt />
        ) : route === '/mqtt' && ready === null ? (
          // Unknown yet whether MQTT is on: wait rather than flash another page.
          <main className="page" />
        ) : route === '/status' ? (
          <Status ready={ready} />
        ) : (
          <Workflows />
        )}
        <StatusBar ready={ready} />
      </div>
    </ReadyContext.Provider>
  )
}
