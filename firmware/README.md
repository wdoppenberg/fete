# The button panel firmware

Three apps, one PlatformIO project, two ESP32-S3 boards.

```
[10 lit buttons] ──GPIO──> [S3: transmitter] ──ESP-NOW──> [S3: receiver] ──USB C──> fete-show
                            on the screen                  on the laptop            --panel <port>
```

| App           | Runs on              | What it does                                    |
| ------------- | -------------------- | ----------------------------------------------- |
| `discover`    | the button board     | Reports which GPIO each button is wired to      |
| `transmitter` | the button board     | Reads the buttons, broadcasts their state       |
| `receiver`    | the laptop's USB     | Repeats what it hears as lines on the USB port  |

The line format between the receiver and the laptop is
[`../crates/fete-input-panel/PROTOCOL.md`](../crates/fete-input-panel/PROTOCOL.md).

## Before you flash anything

**The program already on your button board knows your wiring, and flashing
destroys it.** If you have its source, read the pin numbers out of it and skip
step 1 — that is faster and more reliable than any discovery. If you do not,
step 1 recovers them.

Your board has **two USB-C sockets**. One is wired to the chip itself (native
USB) and one goes through a CH343 UART bridge. `discover` and `transmitter` use
the native socket. This particular receiver board's native socket has already
been found dead, so the `receiver` environment deliberately uses the CH343
socket instead. On Linux those normally appear as `/dev/ttyACM*` (native) and
`/dev/ttyUSB*` (CH343); on macOS use the matching `/dev/cu.*` device.

The devcontainer installs the pinned PlatformIO CLI and pre-builds all three
firmware environments. Outside it, install PlatformIO Core before continuing.
List the ports before and after plugging in one board to identify it:

```sh
pio device list
```

## 1. Find your pins

```sh
cd firmware
pio run -e discover -t upload
pio device monitor
```

The current `BUTTON_PINS` table was discovered from this panel on 2026-08-27,
so skip this step unless the buttons have been rewired or this is a different
panel. If discovery is needed, press each button once, **in the order you want
them numbered**. Each press prints the GPIO it happened on:

```
button 0  ->  GPIO 4
button 1  ->  GPIO 5
...
```

Send any character to print a ready-made `BUTTON_PINS` line. Paste it into
`include/panel_config.h`, replacing the existing table.

Two things discovery cannot do:

- **It cannot find the LED pins.** They are outputs, and nothing can identify an
  output by watching it. Take those from the original program's source. Until
  you have them, leave `LED_PINS` at `-1` and the firmware will not touch the
  LEDs at all — whatever they were doing before continues.
- **It cannot tell you the polarity** if your buttons pull high rather than low.
  If pressing prints nothing but releasing does, flip `BUTTON_PRESSED_LEVEL`.

## 2. Flash the two boards

```sh
pio run -e transmitter -t upload --upload-port /dev/ttyACM0
pio run -e receiver    -t upload --upload-port /dev/ttyUSB0
```

Substitute the paths reported by `pio device list`. Flash one at a time, then
label the boards. On macOS always use the `/dev/cu.*` name, never `/dev/tty.*`:
the latter blocks on open waiting for a carrier signal a USB device never
asserts.

`board_build.arduino.memory_type = qio_opi` in `platformio.ini` is what makes an
octal-PSRAM module boot. With the default it resets in a loop for no visible
reason.

## 3. Run the show

```sh
cargo run -p fete-show -- --panel-list                  # find the receiver
cargo run -p fete-show -- --panel /dev/ttyUSB0 --panel-test --no-video
```

Once every button is proven, remove `--panel-test`. The autopilot is then on,
and panel presses temporarily override it without stopping the unattended show:

```sh
cargo run --release -p fete-show -- \
  --panel /dev/ttyUSB0 --fullscreen --no-hud --no-video
```

## Checking each link on its own

If something does not work, these narrow it down in order.

**The show, with no hardware.** Proves the host half before any board is
involved:

```sh
mkfifo /tmp/fete-panel
python3 tools/fake-panel.py /tmp/fete-panel &
cargo run -p fete-show -- --panel /tmp/fete-panel --panel-test
```

The visual should change every two seconds as it walks buttons 0–9.

**The receiver alone.** With no transmitter running, the frame count should
climb — that means USB, framing and parsing work and only the radio is missing:

```sh
cargo run -p fete-input-panel --example panel-monitor -- /dev/ttyUSB0
```

**Both boards.** Press buttons; the monitor prints a line per press. `panel age`
should stay around a few milliseconds. A rising age means the USB receiver is
alive but its radio link to the transmitter is not. The dropped-packet counter
uses the transmitter's sequence and measures individual ESP-NOW packets.

## What each button does under `--panel-test`

Ten buttons, ten effects, no two alike — so a press is either obviously right or
obviously broken. Test mode also **turns the autopilot off**, so nothing moves
unless you press something.

| Button | Effect                              |
| ------ | ----------------------------------- |
| 0      | sprawl                              |
| 1      | neon                                |
| 2      | yama                                |
| 3      | terebi                              |
| 4      | slime                               |
| 5      | kanban                              |
| 6      | kura                                |
| 7      | next palette                        |
| 8      | re-seed the current visual          |
| 9      | hold macro knob 0 up while pressed  |

There is no blackout in this mapping, on purpose.

Drop `--panel-test` and the buttons take the show mapping: 0 and 1 step through
the visuals, 2 taps tempo, 3 shifts the palette, and 4–9 hold macro knobs 0–5.
Those are deliberately subtler — a knob moves about a third of its range per
half-second press and the autopilot drifts it back — so use test mode to prove
the wiring and the show mapping to judge the feel. A visual chosen from the
panel gets a fresh full autopilot hold before automation changes it again.

## Pairing and range

The transmitter **broadcasts**, so neither board needs the other's MAC. For a
permanent install, put the receiver's MAC in `PEER` in
`src/transmitter/main.cpp` and it stops shouting at the whole room.

Both boards are pinned to `ESPNOW_CHANNEL` in `include/panel_config.h`. Move
both together if the venue's wifi sits on top of you. Both firmware targets
enable Espressif's long-range PHY alongside B/G/N. You are sending eleven bytes
at 50 Hz, so the extra link margin is worth far more than peak bitrate.

Bodies absorb 2.4 GHz, so get the receiver's antenna up with line of sight to
the screen, and keep the panel board's antenna clear of any metal it is
mounted on.
