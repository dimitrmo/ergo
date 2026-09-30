# ergo

ergo is a visual workflow builder for Home Assistant. A workflow starts from
a trigger and runs steps:

- **Triggers:** an entity changing state (in real time), a cron schedule, or
  the Run button.
- **Steps:** publish to MQTT. HTTP calls, HA service calls, and data and logic
  nodes arrive in later versions.

## Getting started

1. Start the add-on and open **ergo** in the sidebar.
2. Click **New workflow** and give it a name.
3. Add a trigger from the left, e.g. **State change**, and pick an entity.
4. Add **MQTT publish**, connect the trigger to it, and set a topic and
   payload. Payloads can use templates such as `{{ trigger.to }}`.
5. Press **Test run** to try the draft, then **Activate** to make it live.

Edits save automatically as a draft. The active version keeps running until
you activate again.

## MQTT

ergo uses the broker from the Mosquitto add-on automatically. Without a
broker, workflows still run, and MQTT nodes fail with a clear error.

## Options

| Option | Default | What it does |
| --- | --- | --- |
| `log_level` | `info` | How much ergo writes to the log |
| `run_retention_days` | `7` | Run history older than this is deleted |
| `run_retention_max` | `1000` | The oldest runs are deleted beyond this count |
| `max_concurrent_runs` | `20` | Runs beyond this are skipped and logged |

## Troubleshooting

- The status bar at the bottom of ergo shows the Home Assistant and MQTT
  connections. Click it for details.
- The add-on log shows every trigger reload, run and error.
- ergo is only reachable through Home Assistant (Ingress); opening its port
  directly is refused by design.
