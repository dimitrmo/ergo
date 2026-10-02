# ergo

ergo is a visual workflow builder for Home Assistant. A workflow starts from
a trigger and runs steps:

- **Triggers:** an entity changing state (in real time), a schedule, an MQTT
  message, or the Run button.
- **Steps:** Home Assistant actions (turn on a light, notify your phone, any
  `domain.action`), **If** to go one way or another, MQTT publish, web
  requests, and data steps that parse, filter, map and compose.

## Home Assistant actions

The **HA action** step lists every action your Home Assistant offers, with the
entities it can act on and the options it takes. Options are a JSON object and
can use templates, e.g. `{"brightness_pct": {{ input.level }}}`.

A workflow isn't triggered again by the state changes its own actions cause,
so "when the light changes, toggle it" can't loop forever. Other workflows do
see those changes.

## If

**If** checks a condition (rules, a template, or JSONata) on the data it
receives and continues from **yes** or **no**. The steps after it get the same
data the If got.

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

The `mqtt_url` option decides whether ergo uses MQTT:

- `auto` (the default) uses the broker the Supervisor knows about, normally
  the Mosquitto add-on, with its address and login.
- A URL such as `mqtt://user:pass@192.168.1.10:1883` uses that broker
  (TLS isn't supported yet).
- `off` turns MQTT off.

When ergo starts, it checks the setting and connects to the broker, waiting
up to 20 seconds. Only if that works does it turn MQTT on: the **MQTT
publish** step, the **MQTT** page and the MQTT status. Otherwise ergo runs
without them, the add-on log and the Status page say why, and workflows that
use MQTT publish report that MQTT is off. Fix the setting (or start the
broker) and restart the add-on.

Once MQTT is on, a broker that goes away later is reconnected automatically;
MQTT steps fail with a clear error until it's back.

### MQTT triggers

The **MQTT message** trigger starts a run when a message arrives on a topic or
a filter (`+` for one level, `#` for everything below), optionally only for an
exact message such as `single`. Retained messages, which a broker replays when
ergo subscribes, don't start runs. **Try it** replays the newest message seen
on the topic, if any.

## Options

| Option | Default | What it does |
| --- | --- | --- |
| `log_level` | `info` | How much ergo writes to the log |
| `mqtt_url` | `auto` | MQTT broker: `auto`, a broker URL, or `off` (see above) |
| `run_retention_days` | `7` | Run history older than this is deleted |
| `run_retention_max` | `1000` | The oldest runs are deleted beyond this count |
| `max_concurrent_runs` | `20` | Runs beyond this are skipped and logged |

## Troubleshooting

- The status bar at the bottom of ergo shows the Home Assistant and MQTT
  connections. Click it for details, including why MQTT is off.
- The add-on log shows every trigger reload, run and error.
- ergo is only reachable through Home Assistant (Ingress); opening its port
  directly is refused by design.
