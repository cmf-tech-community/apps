# AGENTS.md

Community apps for the CMF Watch Pro 2: one Rust crate per app in `apps/<name>/`, built with the
[cmf-watch-sdk](https://github.com/cmf-tech-community/cmf-watch-sdk) at the commit the `Makefile` pins
(`WP2_APPS_REV`). CI builds merged apps and publishes them to the FMC store. README.md is the human guide; this file
is what an agent needs on top.

## Read first

`make sdk` checks the pinned SDK out to `.sdk/wp2-apps`. Its docs match the SDK you build with:

- `.sdk/wp2-apps/docs/app-guide.md`: the manual (app model, `Api`, stock look, storage, phone channel, budgets).
- `.sdk/wp2-apps/docs/abi-1.4.md`: the channel key K, heart rate, pager, stub version.
- `apps/vibration-lab` (template), `apps/ui-gallery` (stock widgets), `apps/sky-hop` (a game): worked examples.

LVGL apps use the system font and the stock look (`cmf_wp2_app::stock`), never LVGL's defaults or an own palette.

## Loop

```bash
make new NAME=my-app     # copy of vibration-lab
make test APP=my-app     # host unit tests
make sim APP=my-app      # simulator window
make build APP=my-app    # target/my-app.wp2app
make check               # store.toml / store.md
```

Emulator and watch targets (`emu`, `run`, `list`) need `CMF_WATCH_UTILS`, `STOCK_NOR` and `WP2_NETSIMD`: README.md.

## Hard rules

1. No writes to a physical watch (`make run ADDRESS=…`, `remove`, firmware) without the owner's explicit yes.
   The simulator and the emulator are fine.
2. No prebuilt `.wp2app`, `.bin`, ELF or firmware in commits; no personal data (MAC, serial, pairing secret, keys,
   GPS tracks, notification text). Placeholder MAC: `AA:BB:CC:DD:EE:FF`.
3. Nothing from cmf-watch-utils (AGPL-3.0) is copied here; this repository is MIT.
4. One app per PR. Every change to an app raises `version` in its `store.toml`, or CI fails.
5. The app stays inside the `Api` table: nothing keeps the motor, the screen or a sensor on after it exits.

## Apps that talk to a phone

Watch side: `send`/`recv` and the `MESSAGE` event (`app-guide.md`, section 4, with its protocol design rules).
Treat every message as untrusted: anyone in BLE range can read the pairing secret and send messages.

Phone side, next to the Nothing X app on the same phone: the channel key K (`abi-1.4.md`, "Writing a host"). No
bind, no "Paired" popup, Nothing X keeps working. What exists today:

- The watch needs the stub with K (`fw-wp2.bin` from 2026-11-13 or later, by OTA) and a loader from 2026-11-13 or
  later. Stock firmware has no channel at all.
- Host implementations: Python (`wp2apps.channel.Channel(channel_key=True)`, Mac) and an iOS one in a private app.
  There is no Android host yet; `connectGatt` next to Nothing X is untested.
- A Mac and a phone can't both be connected: the watch takes one central.

An app that needs a companion says so under `## Requirements` in `store.md`, naming the companion and where to get
it.
