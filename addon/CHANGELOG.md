# Changelog

## 0.1.0

### Added

- Visual workflow editor (React Flow): palette, drag-and-drop, config panel
  built from each node's schema, live entity picker, cron preview.
- Triggers: state change (real time over the HA WebSocket API), cron
  schedule in HA's time zone, manual.
- MQTT publish node with templates, QoS and retain; broker discovered from
  the Mosquitto add-on.
- Drafts and activated versions; test runs of the draft.
- Run history with each node's input and output.
- `/health` for the Supervisor watchdog and `/ready` for component status.
