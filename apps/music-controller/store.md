Control music playback and the phone's music-stream volume from the watch.

The standalone Android **Music Companion** app shows the current track and its notification artwork on the phone. To protect privacy, only generic playback state and volume are sent to the watch; the title, artist and artwork never cross the watch's Bluetooth link. Anyone close enough to read that link can therefore see only whether media is playing and the volume level.

## Requirements

- The [patched firmware](/firmware) with the channel key and the [App Loader](/firmware).
- The standalone [Android Music Companion](https://github.com/boxmol71-gif/music-companion) app. [Download the v0.1.0 companion APK](https://github.com/boxmol71-gif/music-companion/releases/download/v0.1.0/MusicCompanion-0.1.0-debug.apk), then enable its watch bridge and grant notification-listener access.
