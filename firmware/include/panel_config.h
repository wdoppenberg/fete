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
// Your buttons have three wires each, and something on the board is already
// driving them, so these almost certainly exist — but discovery cannot find an
// output pin by watching it. Fill these in from the existing program's source
// if you have it, or leave them at -1 and the firmware simply will not touch
// the LEDs.
static const int LED_PINS[BUTTON_COUNT] = {-1, -1, -1, -1, -1, -1, -1, -1, -1, -1};

// Buttons pull to ground through the internal pull-up, so a press reads LOW.
// Confirmed on this panel: discovery only sees a press this way round, and it
// saw all ten.
static const int BUTTON_PRESSED_LEVEL = LOW;

// LED polarity: HIGH lights it, unless your LEDs sink through the GPIO.
static const int LED_ON_LEVEL = HIGH;

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
