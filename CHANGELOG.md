# Changelog

## v0.2.1

- Linux ARM64 release builds are now available with ALSA audio support.
- Applications now pause and resume correctly when entering or leaving the background.
- Improved network reset reliability by closing stale sockets.
- Screen buffers now resize dynamically when display dimensions change.
- DrawBitmap now handles image rotation and flipping correctly.
- Game controllers are now supported using the following mappings:

| Game controller | MRP key |
| --- | --- |
| D-pad | Directional keys |
| A / B | A / B |
| X / Y | Left / right soft key |
| Start | Select |
| Back | Right soft key |
| Guide | Power |
| L1 / R1 | `*` / `#` |

## v0.2.0

- Added Android support.
- Added a responsive virtual keypad and haptic feedback.
- Added multiline text input with IME support.
- Improved resource loading performance for MRP packages.

## v0.1.0

First release.
