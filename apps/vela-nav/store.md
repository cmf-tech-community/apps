Shows the drive that **Vela** on your phone is navigating: the next turn big at the bottom with its distance, a
heading-up map of the route over the streets around you, which fade out away from it (turn the crown to zoom), the speed limit, the road and your arrival time
with the distance and time left, and an arc along the edge that fills as the drive goes. The screen stays on while a
route is active.

Without a drive, the app shows **Home** and **Work** buttons: a tap asks Vela on the phone to start the drive there
from where you are, with your avoid settings and voice. Vela tells the watch only which of the two are set, never
where they are.

What goes over Bluetooth is the turn, the road name, and the route, the streets around you and your position in
metres from the start of the route: never latitude and longitude, and never the destination's name. The watch's app channel can be read by
any device in Bluetooth range, so a nearby listener could still see the shape of the drive and the road names.

## Requirements

- The [patched firmware](/firmware) (`fw-wp2.bin` from 2026-11-13 or later) and the [App Loader](/firmware)
  (2026-11-13 or later).
- Vela for Android with the watch link, [APK on GitHub](https://github.com/arunk140/esp32-vela/releases/latest)
  ([source](https://github.com/arunk140/esp32-vela)), on the phone that is paired with the watch in Nothing X. In
  Vela: Settings > Nav device > CMF watch test, pick the watch, turn on **Show navigation on the watch**.

Open it from the **Apps** row at the end of the watch's app list before you start driving.
