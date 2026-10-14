# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-14

First public preview for Windows 10 (1903+) and Windows 11.

### Recording
- Record the full screen, a window, or a custom area (drag, exact pixel size, or 16:9 / 9:16 / 1:1 / 4:3 presets).
- Separate tracks for screen, webcam, microphone and system audio, with a webcam preview and level meters.
- Native-resolution capture at 30 / 60 fps with three quality levels; hardware encoders with a software fallback.
- Rebindable global shortcuts with conflict suggestions, a countdown, pause / resume segments, and a floating control bar excluded from capture.
- Cursor highlight (color, size, strength), remembered between recordings.
- Opt-in keystroke capture (shortcuts only, or every key with a warning).
- Crash recovery, safe stop on a full disk, and continued recording when a webcam or microphone is unplugged (the lost range is marked on the timeline).

### Editing
- Auto-polish on stop: background, padding, rounded corners, shadow, automatic zoom, and a smoothed cursor with click ripples.
- Multi-track timeline with split, trim, ripple delete, snapping, track lock, volume and unlimited undo / redo.
- Editable zoom track, webcam picture-in-picture options, text, blur / pixelate, aspect ratios, speed, fades and audio separation.
- Import of common video, audio and image formats, including animated GIF; relinking of missing media; proxies for 4K media.
- Autosave every 60 seconds and 10 rolling backups, with a recovery prompt after a crash.

### Export
- MP4 (H.264, plus HEVC / AV1 when supported) and GIF, presets, and in / out range export.
- Export runs in a separate process with progress, cancellation and a per-project queue.

[0.1.0]: https://github.com/nguyennhuanhle/mochi-studio/releases/tag/v0.1.0
