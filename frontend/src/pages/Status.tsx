import { ago, type Ready } from '../api.ts'
import { PageHeader } from '../components/PageHeader.tsx'
import { TopBar } from '../components/TopBar.tsx'
import './Status.css'

function Row({ ok, name, detail }: { ok: boolean | null; name: string; detail: string }) {
  return (
    <div className="check">
      <span className={`dot ${ok === null ? '' : ok ? 'ok' : 'err'}`} />
      <span className="check-name">{name}</span>
      <span className="quiet">{detail}</span>
    </div>
  )
}

export function Status({ ready }: { ready: Ready | null }) {
  return (
    <>
      <TopBar />
      <main className="page">
        <div className="page-inner">
        <PageHeader title="Status" subtitle="How ergo's connections are doing right now." />
        {!ready ? (
          <Row ok={false} name="Backend" detail="The ergo backend isn't answering /ready." />
        ) : (
          <div className="checks surface">
            <Row
              ok={ready.checks.ha_websocket.ok}
              name="Home Assistant"
              detail={
                ready.checks.ha_websocket.ok
                  ? `Connected to ${ready.checks.ha_websocket.ha_version ?? 'HA'}; last event ${ago(ready.checks.ha_websocket.last_event_at)}`
                  : (ready.checks.ha_websocket.error ?? 'Disconnected')
              }
            />
            <Row
              ok={ready.checks.mqtt.configured ? ready.checks.mqtt.connected : null}
              name="MQTT"
              detail={
                !ready.checks.mqtt.configured
                  ? 'No broker configured. Install the Mosquitto add-on or set ERGO_MQTT_URL.'
                  : ready.checks.mqtt.connected
                    ? `Connected to ${ready.checks.mqtt.broker}`
                    : (ready.checks.mqtt.error ?? 'Disconnected')
              }
            />
            <Row
              ok={ready.checks.database.ok}
              name="Database"
              detail={ready.checks.database.ok ? 'ergo.db is writable' : (ready.checks.database.error ?? '')}
            />
            <Row ok={ready.checks.scheduler.ok} name="Scheduler" detail={`Time zone ${ready.checks.scheduler.time_zone}`} />
            <p className="faint meta">
              ergo {ready.version} · up {Math.floor(ready.uptime_s / 60)} min · overall {ready.status}
            </p>
          </div>
        )}
        </div>
      </main>
    </>
  )
}
