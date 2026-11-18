# AGENTS.md

Community apps for the CMF Watch Pro 2: one Rust crate per app in `apps/<name>/`, built with the
[cmf-watch-sdk](https://github.com/cmf-tech-community/cmf-watch-sdk) at the commit the `Makefile` pins
(`WP2_APPS_REV`). CI builds merged apps and publishes them to the FMC store. README.md is the human guide; this file
is what an agent needs on top.

## Read first

`make sdk` checks the pinned SDK out to `.sdk/wp2-apps`. Its docs match the SDK you build with:

- `.sdk/wp2-apps/docs/app-guide.md`: the manual (app model, `Api`, widget and page, stock look, storage, phone
  channel, budgets, the shipping checklist in section 7).
- `.sdk/wp2-apps/docs/abi-1.4.md`: the channel key K, heart rate, pager, cards, stub version; horizontal pager,
  phone music (`media_key`, `media_bind`) and the 5 s escape for `own_back` apps need the loader of 2026-11-16.
- `apps/vibration-lab` (template, canvas), `apps/ui-gallery` (stock LVGL widgets), `apps/sky-hop` (a game): worked
  examples. More in `.sdk/wp2-apps/sdk/examples` (`hello` and `herdr-agents` have a widget and a page).

**The SDK moves faster than the pin.** The owner publishes new loaders and stubs often, and the pinned docs only know
what existed at `WP2_APPS_REV`. Before telling anyone the watch can't do something, look at what is newer:

```bash
git -C .sdk/wp2-apps fetch --quiet origin && git -C .sdk/wp2-apps log --oneline HEAD..origin/main
```

If a newer commit has it, say so and move the pin first (skill `sdk-update`).

LVGL apps use the system font and the stock look (`cmf_wp2_app::stock`), never LVGL's defaults or an own palette.
`app-guide.md`, "Stock components: SDK and ui-gallery", maps each stock component to its SDK call. Rollers: short and
not infinite (infinite ones draw at 4-6 fps; same section).

## What an app can be

One package can offer several of these; ask which the owner wants before building.

| Surface | How | Loader |
|---|---|---|
| Menu app | the default: a row in Apps, `main` runs until `BUTTON` | any |
| Widget card | `app_features!(WIDGET)`; `api.widget(title, value)` stores text (23 B / 39 B, `\n` = 2nd line) the stub draws on a 344×166 card left of the face. A NAND write: on change, never per frame. A tap opens the app | 2026-11-15 |
| Page | `app_features!(PAGE)`: the app is a whole screen next to the face. `api.on_page()` is `true` there; no `BUTTON` (the app never exits itself), swipe away = `PAUSE`, back = `RESUME` | 2026-11-15 |
| Phone or Mac link | `send`/`recv`, `MESSAGE`: section "Apps that talk to a phone" below | 2026-11-13 for K |
| Phone services | calendar, location, weather, HTTP GET, notifications from FMC Companion over `R1` messages: "FMC Companion" below | 2026-11-13 for K |

The user adds a widget or a page on the watch: long press on the app's row in Apps → "Add widget" / "Add page".
Details: `app-guide.md`, "Widget" and "Page". A "full-screen widget" means a page.

## Loop

```bash
make new NAME=my-app     # copy of vibration-lab
make test APP=my-app     # host unit tests
make sim APP=my-app      # simulator window: canvas apps only
make build APP=my-app    # target/my-app.wp2app
make check               # store.toml / store.md
```

- **The simulator has no LVGL** (`lv_submit` returns `-95`), no widget card and no page (`on_page()` is `false`). An
  LVGL app is tested by host tests of its batch builder (each batch validates against the loader's checker) and in
  the emulator. Pick canvas (`cmf-wp2-ui`) when the simulator matters more than stock widgets: `app-guide.md`,
  "LVGL or `cmf-wp2-ui`?".
- `make build` needs `resvg`, `llvm-objcopy` (rustup `llvm-tools`) and a C linker; `make doctor` lists what is missing.
- Emulator and watch targets (`emu`, `run`, `list`) need `CMF_WATCH_UTILS`, `STOCK_NOR` and `WP2_NETSIMD`: README.md.

## Skills

Step-by-step recipes in `.claude/skills/` (plain Markdown, any agent can follow them):

- `watch-app`: a new app or a change to one, from choosing its surfaces to the PR.
- `sdk-update`: move `WP2_APPS_REV` to a newer cmf-watch-sdk and teach this file what it brought.

## Hard rules

1. No writes to a physical watch (`make run ADDRESS=…`, `remove`, firmware) without the owner's explicit yes.
   The simulator and the emulator are fine.
2. No prebuilt `.wp2app`, `.bin`, ELF or firmware in commits; no personal data (MAC, serial, pairing secret, keys,
   GPS tracks, notification text). Placeholder MAC: `AA:BB:CC:DD:EE:FF`.
3. Nothing from cmf-watch-utils (AGPL-3.0) is copied here; this repository is MIT.
4. One app per PR. Every change to an app raises `version` in its `store.toml`, or CI fails. An SDK bump is a PR
   of its own.
5. The app stays inside the `Api` table: nothing keeps the motor, the screen or a sensor on after it exits.

## Apps that talk to a phone

Watch side: `send`/`recv` and the `MESSAGE` event (`app-guide.md`, section 4, with its protocol design rules).
Treat every message as untrusted: anyone in BLE range can read the pairing secret and send messages. For the same
reason nothing personal goes over the channel: no names, event or message titles, locations. Send counts, flags,
times of day.

Phone side, next to the Nothing X app on the same phone: the channel key K (`abi-1.4.md`, "Writing a host"). No
bind, no "Paired" popup, Nothing X keeps working. What exists today:

- The watch needs the stub with K (`fw-wp2.bin` from 2026-11-13 or later, by OTA) and a loader from 2026-11-13 or
  later. Stock firmware has no channel at all.
- Host implementations: Python (`wp2apps.channel.Channel(channel_key=True)`, Mac) and FMC Companion (below), iOS
  and Android. The Android one (a second GATT client next to Nothing X) is not yet tried on a real watch.
- A Mac and a phone can't both be connected: the watch takes one central.

An app that needs a companion says so under `## Requirements` in `store.md`, naming the companion and where to get
it.

### FMC Companion

The phone app for the watch and fmc.tools, MIT, iOS (SwiftUI) and Android (Compose):
[fmctools/companion](https://github.com/fmctools/companion). It reads the App Loader state over K next to Nothing X,
browses the FMC catalog, installs watch faces (Nothing X closed first), updates the App Loader, and answers watch
apps' requests for phone services (below). Its Apps tab lists the apps on the watch with a switch per service each
asked for.

What a watch app can count on from the companion: [docs/companion.md](docs/companion.md). That covers the Apps tab,
one BLE link with several sessions, iOS background limits (about 10 s per wake), why there are no web modules, and
building your own companion.

**Phone services.** The spec, and the only source of truth for byte layouts, is
[`docs/miniapp-rpc.md`](https://github.com/fmctools/companion/blob/main/docs/miniapp-rpc.md) in the companion repo
(`gh api repos/fmctools/companion/contents/docs/miniapp-rpc.md -H "Accept: application/vnd.github.raw"`); read it
before writing an app that uses one. Its encodings are pinned by `ios/WireKit/Tests/WireKitTests/rpc-vectors.json`
there. `apps/calendar` is the worked example (`src/rpc.rs`, `src/model.rs`: request, reply, cache, retry).

| Method | Id | Cap | Returns |
|---|---|---|---|
| `caps` | `0x01` | none | the method ids this app may call now: tells "allow on your phone" from an empty result |
| `calendar.events` | `0x10` | `calendar` | events for 1-7 days from today (`limit` 1-20): day, start minute, duration, flags, RGB565 colour, title |
| `location` | `0x20` | `location` | lat/lon in 0.1°, fix age, locality name |
| `weather` | `0x30` | `weather` | now (0.1 °C, WMO code, wind, humidity) and up to 7 days (min/max, code, precip %) |
| `http.get` | `0x40` | `http:<host>` | status, length and body (≤ 2048 B, in 121 B chunks) of an `https://` GET, URL ≤ 123 B |
| `notifications` | `0x50` | `notifications` | the notifications shown now, newest first (≤ 10): age, app label, "title: text". Android only |

Both apps implement all six. Only `calendar.events` is used by an app here and tried on a real watch; the rest have
no watch app yet, so expect to find the first bugs.

How it works and what it can't do:

- RPC rides on app messages: every message starts with `R1`, then `Q` (request, watch → phone:
  `R1Q id method args`) or `A` (reply: `R1A id status index total record`). Other prefixes stay the app's own. A
  result of N records comes as N messages; empty or an error is one message with `total` 0.
- The watch pulls: the phone answers the running app only, and nothing is pushed to a closed app. Replies carry no
  link-level request id: one request at a time, ignore replies whose `id` is not your latest.
- Statuses: 0 `OK`, 1 `BAD_REQUEST`, 2 `DENIED` (the user said no, or the OS permission is off), 3 `UNAVAILABLE`
  (no fix, no network, timeout, notifications on iOS), 4 `ASK` (the phone asked the user; show "allow on your
  phone"), 5 `TOO_LARGE` (`http.get`). No answer at all (phone asleep, out of range, companion not running, often on
  iOS): show the cached copy from `store` and ask again after 3 s, then less often.
- Limits: a message is 1-128 bytes, a record ≤ 121; 4 messages in the inbox, so drain `recv` every loop. The phone
  answers within 8 s (iOS gives it about 10 s per wake). Text fields are UTF-8, ≤ 64 bytes; the watch font has ASCII
  and Cyrillic only, anything else draws as `?`. Times are the phone's local day + minute, no zones.
- Grants are per app package, asked on first use: the first call returns `ASK` and the phone posts a notification;
  the user allows or denies on the app's page in the companion's Apps tab.
- Read only. There is no method that writes to the phone: no creating events, no RSVP, no replying to
  notifications, no POST. iOS couldn't RSVP anyway (EventKit can't set your attendee status). A new method is a
  change to `miniapp-rpc.md` and both apps first.
- Personal data: K is readable by anyone in BLE range. Calendar titles and notification text are off by default
  (records come with flag bit 2 and an empty title; show "busy"); a per-app "Show text on the watch" switch turns
  them on. Location is 0.1° only. Event locations, attendees and notes are never sent.
