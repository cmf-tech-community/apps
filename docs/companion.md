# FMC Companion from a watch app's side

What a watch app can count on from the phone, and what it can't. The services and their byte layouts are in
[`docs/miniapp-rpc.md`](https://github.com/fmctools/companion/blob/main/docs/miniapp-rpc.md) in the companion repo.
The companion side of this text is
[`docs/apps.md`](https://github.com/fmctools/companion/blob/main/docs/apps.md) there.

## What the phone answers

Every method, its capability (what the user allows per app) and where it works. "Android" or "iOS" alone: the other
phone answers `UNAVAILABLE` and `caps` doesn't list it. Request and record layouts are in
[`miniapp-rpc.md`](https://github.com/fmctools/companion/blob/main/docs/miniapp-rpc.md); in a watch app, use
[`crates/companion-rpc`](../crates/companion-rpc) instead of building bytes by hand.

| Method | Cap | Phone | |
|---|---|---|---|
| `caps` | none | both | which methods this app may call now |
| `calendar.events` | `calendar` | both | the next 1-7 days of events |
| `reminders`, `reminders.complete` | `reminders` | iOS | open reminders; mark one done |
| `location` | `location` | both | 0.1° and the town |
| `weather` | `weather` | both | now, the next hours (up to 24) and up to 7 days; hours need a companion of 2026-11-20 or later |
| `http.get`, `http.request` | `http:<host>` | both | an HTTPS request to that host, body ≤ 2048 B |
| `notifications` | `notifications` | Android | the notifications shown now |
| `notify` | `notify` | both | a notification on the phone, or "find my phone" |
| `storage.get`, `.put`, `.delete` | none | both | 64 KiB of the app's own data on the phone |
| `phone.battery` | none | both | level, plugged in |
| `media.now_playing`, `media.control` | `media` | Android | the track playing; play, pause, skip |
| `job.set`, `job.delete` | the job's | both | the phone repeats a request while the app is closed and puts the answer into its widget card or start blob |

**Allow on your phone.** The first time an app calls a method, the phone answers `ASK` and shows a notification. The
app says so ("Allow Reminders in FMC Companion on your phone") and asks again every few seconds. The user allows or
denies it on the app's page in the Apps tab; a denied method answers `DENIED` from then on, and the app should say
where to turn it back on.

**Titles stay on the phone.** Calendar and reminder titles and notification text come empty, with a flag, unless the
user turns on "Show text on the watch" for that app: anyone in Bluetooth range can read the channel. Show a
placeholder ("busy", "Reminder") instead.

**Newer watches only.** `job.set` delivers its results with `PUSH`, which needs the patched firmware's **stub 8**
(published 2026-11-19 as `1.0.0.73-wp2.11`); the same update brings 16 app slots instead of 8. On an older stub the
phone runs the job and throws the result away. The order of the Apps menu is set by the user in FMC Companion; an app
can't change it.

## The Apps tab

The companion has an Apps tab on iOS and Android. It lists the apps on the watch, plus the ones that asked for
something but are gone from the watch now. Each app's page has a switch for every service it asked for. When an app
asks for something new, the phone shows a notification and the request waits on that page.

## One link, several sessions

There's one BLE link, the one Nothing X holds, but more than one client can use it. Android shares the LE link between
apps' GATT clients, so the companion connects next to Nothing X without a bind or a popup. The companion already runs
two sessions on it: one answers your app's requests, the other reads the watch's state.

The catch is that replies don't carry a request id. Every session sees every reply, so requests have to go one at a
time. The companion does that between its own sessions. Another host sending its own requests at the same moment can
still take a reply, and nothing guards against that.

Installing a watch face is the exception. The stock session binds the watch, so the companion stops answering apps
during the install and the user closes Nothing X first. Expect no answers for that stretch.

## iOS in the background

With `bluetooth-central`, iOS wakes the companion on each BLE notify from the watch and gives it about 10 seconds.
That's why every method answers within 8 s. Your app should show its cached copy and retry, not wait.

- Swiped away by the user: the companion doesn't run at all until it's opened again.
- Killed by the system: it's gone until the next launch. CoreBluetooth state restoration could fix this and isn't used
  yet.
- After a reboot and before the first unlock, the companion can't read K from the keychain.
- Nobody knows yet whether a notify on the link Nothing X holds wakes the companion at all. The companion's DEBUG Wake
  probe checks it, with `wake-probe` from the SDK examples on the watch, and it hasn't been run on hardware. If it
  doesn't wake, services on iOS only work while the companion is open.

## Why not web modules

The idea came up to do it like KernelSU: each app ships config plus a web UI, and the companion is a JS bridge to a
web renderer. It doesn't fit here.

The phone side runs in the background almost all the time. A web UI needs its screen open, so when your app asks for
the weather with the phone in a pocket, nothing is there to answer. That's the hard part, and a web layer just sits
next to it.

Then there's the App Store. Downloading third-party JS that changes what the app does runs into guideline 2.5.2.
KernelSU never goes through review. The companion does.

Permissions are the third problem. Each watch app gets its own grant for calendar, location or notifications, and
native code checks the OS permission before any service runs. A JS bridge would have to expose all of that again and
keep modules from getting around it. A fixed list of methods is much easier to get right.

So the phone side stays a small set of native services. If your app needs one that's missing, propose a new method in
`miniapp-rpc.md`.

## Your own companion

If you need much more than the services give, build your own phone app on the same wire protocol. In the companion
repo, take the transport as it is: `wire/Session.kt`, `wire/Frame.kt` and `BleLink.kt` on Android, `ios/WireKit` on
iOS. Then write your own RPC and services next to it, using `wire/Rpc.kt` and `Services.kt` as the pattern, and check
the encoding against `ios/WireKit/Tests/WireKitTests/rpc-vectors.json`. The Android link isn't tested on a real
watch yet.

Your app's `store.md` then names that companion under `## Requirements`.
