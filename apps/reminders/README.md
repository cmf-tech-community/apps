# Reminders

The iPhone's open reminders on the CMF Watch Pro 2, from [FMC Companion](https://github.com/fmctools/companion)'s
`reminders` and `reminders.complete` ([miniapp-rpc.md](https://github.com/fmctools/companion/blob/main/docs/miniapp-rpc.md)),
through [`crates/companion-rpc`](../../crates/companion-rpc). LVGL, stock look; a menu app and a widget card.

| File | |
|---|---|
| `src/model.rs` | no `Api`: the list call and the complete call, retry and back-off, the cache blob, sections and texts |
| `src/screen.rs` | the list and "Done?" screens, host-tested batch by batch against the loader's checker |
| `src/main.rs` | the device loop: events, the buzz, storage, the widget |

What it does:

- At start the cached list shows at once (`store`, `M1` blob, ≤ 1.5 KB), then the phone is asked; again every 3
  minutes while open, on `RESUME`, and with back-off from 3 s while the phone is silent.
- Sections: overdue (before today, or today with its time passed) in red, today, tomorrow, later days, no date. The
  phone's `day` counts from the day of its answer, so a cached list shifts with the date.
- A tap asks "Done?"; Done removes the row, buzzes 60 ms and queues `reminders.complete` (up to 8 queued, kept in
  the cache until the phone answers). `BAD_REQUEST` (done or deleted meanwhile), `DENIED` and `ASK` settle it too;
  then the list is fetched again, which tells the truth. `UNAVAILABLE`: tried again in 30 s.
- Empty list: `ASK` → "Allow Reminders in FMC Companion on your phone", `DENIED` → where to turn it on,
  `UNAVAILABLE` → "Reminders need an iPhone" (Android answers that), no answer → "Phone not reachable", else
  "All done". With a cached list those show as a line under it.
- Titles hidden by the phone (flag bit 2, the default) show as "Reminder" and the time.
- Widget: "Reminders", "N due today" (overdue included) and the next one; written only when the text changes.

No page: the list needs the crown and taps, and a page can't leave on `BUTTON`.

```bash
make test APP=reminders
make build APP=reminders
```

Run so far: host tests, and the emulator (stub and loader at the pinned SDK) with a fake phone on the channel that
answers `ASK`, then a list, and takes `reminders.complete`: the "allow" screen, the list, the crown, "Done?", the
completion with the right id and the list fetched again, the widget record, and the cached list on reopening without
the phone (the shots). Not yet on a real watch or with the real companion.
