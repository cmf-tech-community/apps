Shows the drive that **Vela** on your phone is navigating: the next turn with its distance and road, a heading-up
map of the route around you (turn the crown to zoom), the speed limit, and your arrival time with the distance and
time left. The screen stays on while a route is active.

What goes over Bluetooth is the turn, the road name, and the route and your position in metres from the start of
the route: never latitude and longitude, and never the destination's name. The watch's app channel can be read by
any device in Bluetooth range, so a nearby listener could still see the shape of the drive and the road names.

## Requirements

- The [patched firmware](/firmware) (`fw-wp2.bin` from 2026-11-13 or later) and the [App Loader](/firmware)
  (2026-11-13 or later).
- Vela for Android with the watch link, [APK on GitHub](https://github.com/arunk140/esp32-vela/releases/latest)
  ([source](https://github.com/arunk140/esp32-vela)), on the phone that is paired with the watch in Nothing X. In
  Vela: Settings > Nav device > CMF watch test, pick the watch, turn on **Show navigation on the watch**.

Open it from the **Apps** row at the end of the watch's app list before you start driving.
