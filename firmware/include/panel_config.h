// Everything both boards have to agree on, and the two pin tables you need to
// fill in for your own wiring.
#pragma once

#include <Arduino.h>

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

// How many buttons the panel has.
static const size_t BUTTON_COUNT = 10;

// The GPIO each button's switch is wired to.
//
// Found with the `discover` app on 2026-08-27, pressing the buttons in the
// order they are numbered here. They sit on GPIO 4-13, but not in pin order —
// the panel is wired to suit the physical layout, which is why these had to be
// discovered rather than assumed.
//
// Pins to keep clear of on an ESP32-S3-N16R8:
//   33-37      taken by the octal PSRAM
//   19, 20     native USB D-/D+
//   0, 3, 45, 46  strapping pins; 0 held low at boot enters download mode, so a
//                 button there stops the board starting if anyone leans on it
static const int BUTTON_PINS[BUTTON_COUNT] = {12, 7, 6, 5, 4, 10, 11, 9, 8, 13};

// The GPIO each button's LED is wired to, or -1 for "not wired / leave alone".
//
// Found with the `led-discover` app on 2026-08-28: it blinks one GPIO at a time
// and waits for you to press whichever button blinked, so the press that answers
// "this one" also says which button it is. Discovery proper cannot do this — it
// watches pins, and nothing identifies an output by watching it.
//
// Three of these are pins you would never pick on purpose, and they are the
// reason the panel looked broken before anyone touched the firmware:
//
//   43, 44  UART0 TX/RX. Harmless here — the console is the native USB CDC port
//           — but **TX idles high**, so button 0's LED was lit from the moment
//           the board powered up, by the pin's default function rather than by
//           any code. That was the whole of "only the first button lights up".
//   3       a strapping pin, sampled at reset and free afterwards.
//
// pinMode(OUTPUT) hands each pin to the GPIO matrix and the default function
// stops driving it. The ROM's boot log still blips button 0 for a moment at
// every reset, before setup() runs; nothing can be done about that from here.
static const int LED_PINS[BUTTON_COUNT] = {43, 3, 17, 16, 15, 41, 42, 2, 1, 44};

// Buttons pull to ground through the internal pull-up, so a press reads LOW.
// Confirmed on this panel: discovery only sees a press this way round, and it
// saw all ten.
static const int BUTTON_PRESSED_LEVEL = LOW;

// LED polarity: HIGH lights it, unless your LEDs sink through the GPIO.
// Confirmed on this panel — every LED answered the high phase of the sweep.
static const int LED_ON_LEVEL = HIGH;

// Whether the panel sits lit and a press puts a button *out*, rather than
// sitting dark and a press lighting one.
//
// The panel lives in the dark next to a DJ. A dark panel is a panel nobody can
// find, and these LEDs are plain on/off GPIOs with no brightness to modulate, so
// the only signal a press can carry is the one the standing light gives up. Set
// this false for the other way round.
static const bool LED_IDLE_LIT = true;

// ---------------------------------------------------------------------------
// Radio
// ---------------------------------------------------------------------------

// Both boards must agree. Move both together if the venue's wifi sits on top.
static const uint8_t ESPNOW_CHANNEL = 1;

// ---------------------------------------------------------------------------
// Wire format
// ---------------------------------------------------------------------------

static const uint32_t PACKET_MAGIC = 0x46455445;  // "FETE"
static const uint8_t PACKET_VERSION = 1;

// Frames per second, on the air and out of the USB port.
static const uint32_t SEND_INTERVAL_MS = 20;

// How long a reading must hold still before it counts as a press.
static const uint32_t DEBOUNCE_MS = 15;

struct __attribute__((packed)) PanelPacket {
  uint32_t magic;
  uint8_t version;
  uint16_t seq;
  uint32_t buttons;
};
