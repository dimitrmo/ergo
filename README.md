<p align="center">
  <img src="branding/logo.png" width="96" alt="Ergo logo">
</p>

<h1 align="center">Ergo</h1>

<p align="center"><em>Something happens at home. Ergo, something gets done.</em></p>

Ergo is a lightweight visual workflow builder for [Home Assistant](https://www.home-assistant.io/),
in the spirit of n8n but made for the home: a Rust backend and a React Flow editor, packaged as a
Home Assistant add-on that lives in your sidebar.

- **Triggers:** an entity changing state (in real time, over HA's WebSocket API), a schedule in
  HA's time zone, or a button press.
- **Steps:** publish to MQTT. HTTP requests, HA actions, data and logic steps are on the roadmap.
- **Built to be friendly:** workflows read as sentences, steps are added with **+**, schedules
  are picked from presets, and **Try it** shows what each step did.
- **Built to be safe:** edits are drafts until you go live, runs are recorded step by step,
  and the UI is reachable only through Home Assistant.

## Install

1. In Home Assistant, open **Settings → Add-ons → Add-on store → ⋮ → Repositories** and add
   `https://github.com/dimitrmo/ergo`.
2. Install **Ergo**, start it, and open **Ergo** in the sidebar.
3. For MQTT steps, install the Mosquitto broker add-on; Ergo finds it automatically.

The add-on documentation (options, troubleshooting) is in [`addon/DOCS.md`](addon/DOCS.md).

## How it fits together

```
Home Assistant ── WebSocket ──► ergo (Rust) ──► MQTT broker
                                   ▲
HA sidebar ── Ingress ──► nginx ───┘  (static UI + /api proxy, Ingress-only)
```

| Part | What it is |
| --- | --- |
| `backend/ergo-core` | Workflow model, validation, templating (MiniJinja), engine |
| `backend/ergo-nodes` | Node types: state, schedule and manual triggers; MQTT publish |
| `backend/ergo` | The binary: HA client, MQTT, triggers, SQLite storage, REST API |
| `frontend` | Vite + React + React Flow editor, built as static files |
| `addon` | The HA add-on: Dockerfile, nginx config, `config.yaml`, docs |
| `dev` | Local Home Assistant + Mosquitto, onboarding and deploy scripts |
| `branding` | Logo, icons and the animated `ErgoLogo` component |

Workflows, versions and run history live in Ergo's own SQLite database (`/data/ergo.db`), which
HA backups include. Run history is kept for 7 days and at most 1,000 runs by default.

## Development

Requirements: Rust (stable), Node 24+, Docker, and [`cross`](https://github.com/cross-rs/cross)
for building the add-on.

```sh
make dev-up          # start a throwaway Home Assistant (:8123) and Mosquitto (:1883)
make dev-bootstrap   # onboard it (user: dev) and write .env with a token
make dev             # run the backend against it (:8100)
make ui-dev          # run the editor with hot reload (:5173)
make dev-sidebar     # optional: add Ergo to the dev HA's sidebar
make mqtt-watch      # optional: print every MQTT message
```

Checks, as CI runs them:

```sh
make lint
make test
```

### Deploying to your own Home Assistant

```sh
echo 'ERGO_DEPLOY_HOST=root@homeassistant.local' >> .env
make deploy ARCH=aarch64   # or amd64
```

This cross-compiles the backend, builds the UI, copies the add-on to `/addons/ergo` over SSH
and installs or rebuilds it there. The Pi only copies files, so the rebuild takes seconds.

### Releases

Run `make bump BUMP=patch|minor|major|X.Y.Z` to set the version of the backend, frontend and
add-on, fill in the new section of `addon/CHANGELOG.md`, and merge to `master`. When CI passes on a
version that has no `vX.Y.Z` tag yet, it pushes both images to GHCR, tags the commit and creates
the GitHub release; Home Assistant then offers the update.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
