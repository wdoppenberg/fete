#!/usr/bin/env python3
"""Pretend to be the receiver board, so the show can be tested with no hardware.

    mkfifo /tmp/fete-panel
    tools/fake-panel.py /tmp/fete-panel            # walks through the buttons
    tools/fete-panel.py /tmp/fete-panel --hold 3   # holds button 3 down

Writes the same lines the receiver firmware writes — see
`crates/fete-input-panel/PROTOCOL.md`. Opening a fifo for writing blocks until
something opens the other end, so starting this before the show is fine: it
waits.
"""

import argparse
import sys
import time

# Frames per second, matching the firmware.
RATE = 50
# How long each button is held, and the gap before the next one, in seconds.
PRESS = 0.4
GAP = 2.0


def frames(args):
    """Yield a button mask for each frame, forever."""
    seq = 0
    start = time.monotonic()
    while True:
        elapsed = time.monotonic() - start
        if args.hold is not None:
            mask = 1 << args.hold
        else:
            # Walk the buttons: press one, pause, press the next.
            slot = int(elapsed // GAP) % args.buttons
            within = elapsed % GAP
            mask = (1 << slot) if within < PRESS else 0
        seq = (seq + 1) & 0xFFFF
        yield mask, seq


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", help="fifo to write to")
    parser.add_argument("--hold", type=int, help="hold one button down instead of walking")
    parser.add_argument("--buttons", type=int, default=10, help="how many buttons to walk")
    args = parser.parse_args()

    print(f"opening {args.path} — waiting for the show to open the other end", file=sys.stderr)
    with open(args.path, "w") as fifo:
        print("connected", file=sys.stderr)
        last = None
        for mask, seq in frames(args):
            # `5` is a plausible age: the receiver heard the panel 5 ms ago.
            fifo.write(f"P {mask:08x} {seq} 5\n")
            fifo.flush()
            if mask != last:
                pressed = [i for i in range(32) if mask & (1 << i)]
                print(f"buttons {pressed}" if pressed else "released", file=sys.stderr)
                last = mask
            time.sleep(1 / RATE)


if __name__ == "__main__":
    try:
        main()
    except (BrokenPipeError, KeyboardInterrupt):
        print("\nstopped", file=sys.stderr)
