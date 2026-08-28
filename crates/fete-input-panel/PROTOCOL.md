# The panel protocol

Two ESP32-S3 boards and a serial port.

```
[buttons] ── GPIO ──> [S3: panel] ── ESP-NOW ──> [S3: receiver] ── USB CDC ──> fete-show
              on the screen                      behind the booth              --panel <port>
```

The host end is `crates/fete-input-panel`. This file is the contract the
firmware has to meet; nothing else in the repo needs to change to support a
different panel.

## The line format

One frame per line, ASCII, `\n` terminated, at 115200 baud (a fiction over
native USB CDC, but harmless to set).

```
P <mask> <seq> <age>
```

| Field  | Type          | Meaning                                                     |
| ------ | ------------- | ----------------------------------------------------------- |
| `mask` | hex, up to 8 digits | Bit *n* set means button *n* is **currently held**.    |
| `seq`  | decimal u16   | Wrapping counter, incremented once per frame sent.           |
| `age`  | decimal u32   | Milliseconds since the receiver last heard from the panel.   |

Lines beginning with `#` are comments and are ignored, so the firmware can log
to the same port while you are bringing it up.

```
# receiver up, ch 6, LR mode
P 00000000 1 4
P 00000005 2 9      <- buttons 0 and 2 held
P 00000004 3 7      <- button 0 released
```

## Rules the firmware must follow

**Send absolute state, never edges or toggles.** A dropped "button 3 toggled"
leaves the show inverted until somebody presses it again. A dropped "these
buttons are down" is corrected by the next frame. This is the single most
important line in this document — the radio *will* drop packets in a room full
of people, because 2.4 GHz is absorbed by bodies.

**Send continuously, not only on change.** 50 Hz is plenty. The host uses the
arrival of frames as its liveness signal: silence for 1.5 s means the link is
down, everything is treated as released, and the autopilot takes the knobs
back.

**Debounce on the panel board, not here.** The host trusts the mask.

**`age` is the far half of the link.** The host can tell the receiver is alive
because lines are arriving, but only the receiver knows whether the panel out
on the screen is still talking. Report the real figure; if it exceeds 1.5 s the
host treats every button as released rather than latching whatever was held
when the panel vanished.

## Radio notes

- **ESP-NOW**, not an AP association. Connectionless, ~1–2 ms, no reconnect
  storm when the link drops.
- **Enable long-range mode** on both ends
  (`esp_wifi_set_protocol(..., WIFI_PROTOCOL_LR)`). You are sending a dozen
  bytes at 50 Hz; trading bitrate for link margin is free here and buys a lot
  in a crowded room.
- **Pin the channel**, both ends the same, away from the site wifi.
- **Get the receiver's antenna up high** with line of sight to the screen. If
  the panel board is against a metal screen frame, use a module with a U.FL
  connector and stand the antenna off the metal.
- ESP-NOW's send callback gives per-packet delivery status. Drive an LED from
  it so somebody on site can see the link is alive without a laptop.

## Pin notes

Wiring, pin tables and the traps on an N16R8 module live with the firmware, in
[`../../firmware/README.md`](../../firmware/README.md) and
`firmware/include/panel_config.h`.

## Reserved

`L <mask>` host → receiver, to light the buttons, is deliberately unimplemented
on the host side. The format is reserved so firmware can be written against it
without a later change of meaning.
