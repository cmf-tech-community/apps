# Contributing

One app per pull request, in `apps/<name>/`. A PR that changes an app raises its `version` in `store.toml`; CI
checks that, and builds and tests the app.

A review checks:

- **It builds from source here.** No prebuilt `.wp2app`, `.bin`, ELF or firmware file in the PR; the store gets what
  CI builds. Images for the icon and screenshots are fine.
- **It stays inside the app.** Only the `Api` table and the SDK, no writes to firmware or flash outside the app's own
  storage, nothing that keeps the motor or the screen on after the app exits.
- **No personal data.** No MAC addresses, serial numbers, pairing secrets or keys, GPS tracks, notification contents
  or real names other than the author's own. Placeholder MAC: `AA:BB:CC:DD:EE:FF`.
- **No copied code under an incompatible license.** This repository is MIT. In particular nothing from
  cmf-watch-utils (AGPL-3.0).
- **The listing tells the truth.** `store.md` says what the app does and what it needs (`## Requirements`). An app
  is listed as soon as it is merged, so merge it when it's ready.
- **It was run.** In the emulator or on your own watch; say which in the PR, with a screenshot.

Contributions are MIT-licensed, like the rest of the repository.
