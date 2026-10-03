<p align="center">
  <img src="branding/logo.png" width="96" alt="Ergo logo">
</p>

<h1 align="center">Ergo</h1>

<p align="center"><em>Something happens at home. Ergo, something gets done.</em></p>

<p align="center">
  <strong>The friendliest way to automate your home with Home Assistant.</strong><br>
  No code and no YAML. Just say what should happen.
</p>

---

Ergo lives in your Home Assistant sidebar. With it you build automations by snapping steps
together, and each one reads like a plain sentence:

> **When** the front door opens after sunset, **turn on** the hallway light and **send** a
> notification to my phone.

If you can describe it, you can build it.

<p align="center">
  <img src="docs/screenshots/workflows.png" alt="Your workflows, each summed up in a plain sentence">
</p>

## Everything you can build with

Every workflow starts with **one trigger** and then runs as many **steps** as you like.

### 4 ways to start

| Trigger | Starts the workflow… | For example |
| --- | --- | --- |
| 🔄 **State change** | when a light, sensor, switch or person changes | *When anyone gets home…* |
| ⏰ **Schedule** | at set times: every morning, on weekdays, every few minutes | *Every weekday at 7:00…* |
| 📡 **MQTT message** | when a message arrives, like a smart button press | *When the bedside button is double-pressed…* |
| 👆 **Manual** | only when you press **Try it** | *Whenever I want to run "movie night"…* |

### 13 kinds of steps

| Step | What it does |
| --- | --- |
| 🏠 **HA action** | Turns on a light, sets a thermostat, locks a door: any Home Assistant action |
| 🔔 **Notify** | Sends a notification to your phone through the Home Assistant app, with a title and a message |
| 💻 **Web push** | Pops up a notification titled *Ergo* in your browser, on any computer or phone that turned them on; choose how urgent it is |
| 🔀 **If** | Goes one way when something is true and another way when it isn't: a value, the time, the day of the week, or the sun |
| ⏱️ **Delay** | Waits a few seconds, minutes or hours before the next step |
| ⏳ **Wait until** | Waits for something to happen, like a door closing, and takes another path if it doesn't happen in time |
| 📤 **MQTT publish** | Sends a message to your smart gadgets |
| 🌐 **Web request** | Calls a web service or webhook, such as IFTTT or a Discord bot |
| ⬇️ **Download** | Fetches a feed, file or web page |
| 🧩 **Parse** | Turns what you downloaded into information the next steps can use |
| 🔍 **Filter** | Keeps only the items you care about |
| 🗂️ **Map** | Picks out and renames just the details you need |
| ✍️ **Compose** | Writes a message using information from earlier steps |

## Why you'll love it

### ✨ So simple anyone at home can use it
- **Workflows read like sentences.** Every automation is summed up in plain English, so you
  can tell what it does at a glance.
- **Press +, pick what's next.** Big, clear choices with a short explanation for each. No
  menus to dig through.
- **Pick devices by name.** Search your lights, sensors, switches and people by name. You
  never have to remember an ID.
- **Schedules without the head-scratching.** Choose *Every day*, *Weekdays*, *Weekends*,
  *Once a week*, *Every hour* or *Every few minutes*, set a time, and Ergo shows exactly when
  it will run next.

### ⚡ Start automations any way you like
- **When something changes:** a light turns on, a door opens, a sensor reads a new value, or
  someone gets home. Ergo reacts instantly.
- **At set times:** wake-up routines, bedtime routines, weekly reminders.
- **When you press a button,** from a smart button, a remote, or anything else that sends
  MQTT messages.
- **Only when you say so:** keep a workflow on standby and run it yourself.

### 🏠 Do just about anything
- **Control your whole home:** lights, thermostats, covers, media players, scenes and
  notifications. Anything Home Assistant can do, Ergo can do.
- **Make decisions:** *if* it's raining, close the blinds; *otherwise* open them. Each choice
  can lead to its own steps.
- **Only at the right time:** *after sunset*, *before sunrise*, *between 22:00 and 07:00*, or
  *on weekdays*. Just pick it; no formulas.
- **Take your time:** turn the hallway light off *5 minutes later*, or *wait until* the garage
  door closes and tell you if it doesn't.
- **Get notified:** pick your phone, write a title and a message, done.
- **Notifications in your browser too:** press *Turn on here* on any computer, and Ergo can
  pop up notifications there, no app needed. Urgent ones stay on screen until you dismiss them.
- **Reach beyond your home:** fetch the weather, a news feed, or any web service, and use
  what comes back.
- **Write your own messages:** build notifications from live information, like *"The garage
  has been open for 10 minutes."*
- **Pick out what matters:** keep only the items you care about from a feed or list and
  ignore the rest.

### 🧪 Try before you trust
- **Try it** runs a workflow right away and shows what each step did, step by step.
- **Your changes are drafts until you go live,** so a half-finished edit never touches the
  running version.
- **Run history** keeps a record of every time a workflow ran, what it saw, and what it did,
  which makes "why did the lights do that?" easy to answer.

### 🛡️ Safe by design
- **No runaway loops.** A workflow is never set off by its own actions, so "when the light
  changes, toggle it" can't flicker forever.
- **Private to your Home Assistant.** Ergo can only be opened from inside Home Assistant, by
  people who can already sign in.
- **Backed up with everything else.** Your workflows are saved in Home Assistant's regular
  backups.

### 🧰 Handy extras
- **Turn workflows on and off** with a single switch. Nothing gets deleted.
- **Duplicate** a workflow to make a variation in seconds.
- **Export and import** workflows to keep a copy or share them with a friend.
- **A live view of MQTT messages,** so you can see what your buttons and gadgets are saying
  and send a message again with one click.
- **A status bar** that always shows whether everything is connected, and tells you in plain
  words if something isn't.

## Take a look

### Building a workflow

| | |
| --- | --- |
| ![Pick how a workflow starts](docs/screenshots/trigger-chooser.png) **Pick how it starts.** Four big, friendly choices. | ![Pick what happens next](docs/screenshots/step-chooser.png) **Pick what happens next.** Every step explains itself. |
| ![A workflow on the canvas](docs/screenshots/editor.png) **See the whole thing at a glance.** Each step reads like a sentence. | ![Yes and no branches](docs/screenshots/if-branches.png) **Make decisions.** Go one way or the other with **If**. |
| ![Name a new workflow](docs/screenshots/new-workflow.png) **Start fresh in seconds.** Give it a name and go. | ![Workflow list](docs/screenshots/workflows.png) **All your workflows in one place.** Switch them on and off with a tap. |

### Delays, waiting, notifications and good timing

| | |
| --- | --- |
| ![Garage left open](docs/screenshots/garage.png) **Garage left open.** Wait until it closes; if it doesn't, and it's night, send a notification. | ![Hallway night light](docs/screenshots/night-light.png) **Hallway night light.** If the sun is down, turn the light on, wait 5 minutes, turn it off. |
| ![Wait until](docs/screenshots/wait-until.png) **Wait until** something reaches a state, and choose when to give up. | ![Delay](docs/screenshots/delay.png) **Delay** for seconds, minutes or hours. |
| ![Time window](docs/screenshots/if-time.png) **Only between certain times,** even across midnight. | ![After sunset](docs/screenshots/if-sun.png) **After sunset or before sunrise,** using your home's own sun times. |
| ![Notify](docs/screenshots/notify.png) **Notify** a phone or Home Assistant with a title and a message. | ![Web push](docs/screenshots/web-push.png) **Web push** to every browser that turned notifications on, with an urgency. |
| ![All steps](docs/screenshots/step-chooser.png) **Thirteen kinds of steps,** each explained in a sentence. | |

### Setting up each step

| | |
| --- | --- |
| ![Watch a device](docs/screenshots/state-trigger.png) **Watch any device** and choose the change you care about. | ![Search your devices](docs/screenshots/entity-picker.png) **Find devices by name,** with their current state. |
| ![Control your home](docs/screenshots/ha-action.png) **Control your home:** pick an action and what it acts on. | ![Schedules made simple](docs/screenshots/schedule.png) **Schedules without the guesswork,** plus the next times it will run. |
| ![Conditions](docs/screenshots/if-panel.png) **Set a condition** with simple rules. | ![Write a message](docs/screenshots/compose-output.png) **Write messages** with live information and see the result. |

### Testing and history

| | |
| --- | --- |
| ![What a step received](docs/screenshots/compose-input.png) **See what each step received** and what it did. | ![Run history](docs/screenshots/history.png) **Every run is recorded,** whatever started it. |
| ![Replaying a run](docs/screenshots/history-run.png) **Replay any run** on the canvas, step by step. | ![Run now](docs/screenshots/run-now-confirm.png) **Run manual workflows** from the list, with a quick check first. |

### Behind the scenes

| | |
| --- | --- |
| ![MQTT messages](docs/screenshots/mqtt.png) **A live view of MQTT messages** from your buttons and gadgets. | ![Send an MQTT message](docs/screenshots/mqtt-publish.png) **Send a message yourself** to test a device. |
| ![Database](docs/screenshots/database.png) **Look inside everything Ergo stores,** read-only. | ![Run records](docs/screenshots/database-runs.png) **Browse past runs** in detail. |
| ![Status](docs/screenshots/status.png) **Check the connections** at a glance. | ![Delete safely](docs/screenshots/delete-confirm.png) **No accidents.** Live workflows can't be deleted, and Ergo always asks first. |

## Install in two minutes

1. In Home Assistant, open **Settings → Add-ons → Add-on store**, click the **⋮** menu
   (top right) and choose **Repositories**.
2. Paste `https://github.com/dimitrmo/ergo` and click **Add**.
3. Find **Ergo** in the store, click **Install**, then **Start**.
4. Open **Ergo** from the sidebar and create your first workflow.

Have smart buttons or gadgets that use MQTT? Install the **Mosquitto broker** add-on too, and
Ergo will find it automatically.

Want notifications in your browser? They need Home Assistant to be opened over **HTTPS** (for
example through Home Assistant Cloud or your own certificate), because browsers only allow them
on secure pages.

Need help? See the [add-on guide](addon/DOCS.md).

---

<p align="center">Free and open source under the <a href="LICENSE-MIT">MIT</a> or
<a href="LICENSE-APACHE">Apache-2.0</a> licence.</p>
