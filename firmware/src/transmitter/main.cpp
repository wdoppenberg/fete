// The button board: reads the buttons and broadcasts their state.
//
// Sends absolute state at a fixed rate rather than edges. A dropped "button 3
// went down" leaves the show wrong until somebody presses it again; a dropped
// "these buttons are down" is corrected 20 ms later. See
// ../../crates/fete-input-panel/PROTOCOL.md.
//
// Pins live in include/panel_config.h. Run the `discover` app first.

#include <Arduino.h>
#include <WiFi.h>
#include <esp_now.h>
#include <esp_wifi.h>

#include "panel_config.h"

// Broadcast, so neither board needs to know the other's MAC address. One pair
// in a room is the whole design; for a permanent install, put the receiver's
// MAC here and it stops shouting at everyone.
static uint8_t PEER[6] = {0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF};

static uint16_t sequence = 0;
static uint32_t stable_mask = 0;
static int last_reading[BUTTON_COUNT];
static uint32_t last_change_ms[BUTTON_COUNT];
static uint32_t last_send_ms = 0;

// Read the pins, and fold anything that has held still into the stable mask.
static void debounce() {
  const uint32_t now = millis();
  for (size_t i = 0; i < BUTTON_COUNT; ++i) {
    const int reading = digitalRead(BUTTON_PINS[i]);
    if (reading != last_reading[i]) {
      last_reading[i] = reading;
      last_change_ms[i] = now;
      continue;
    }
    if (now - last_change_ms[i] < DEBOUNCE_MS) {
      continue;
    }
    const uint32_t bit = 1UL << i;
    if (reading == BUTTON_PRESSED_LEVEL) {
      stable_mask |= bit;
    } else {
      stable_mask &= ~bit;
    }
  }
}

// Light a button while it is held, for whichever LEDs are wired.
static void update_leds() {
  for (size_t i = 0; i < BUTTON_COUNT; ++i) {
    if (LED_PINS[i] < 0) {
      continue;
    }
    const bool held = stable_mask & (1UL << i);
    digitalWrite(LED_PINS[i], held ? LED_ON_LEVEL : !LED_ON_LEVEL);
  }
}

void setup() {
  Serial.begin(115200);

  for (size_t i = 0; i < BUTTON_COUNT; ++i) {
    pinMode(BUTTON_PINS[i], INPUT_PULLUP);
    last_reading[i] = digitalRead(BUTTON_PINS[i]);
    last_change_ms[i] = 0;
    if (LED_PINS[i] >= 0) {
      pinMode(LED_PINS[i], OUTPUT);
      digitalWrite(LED_PINS[i], !LED_ON_LEVEL);
    }
  }

  WiFi.mode(WIFI_STA);
  WiFi.disconnect();
  // Keep ordinary B/G/N rates available and add Espressif's long-range PHY.
  // Both ends set the same bitmap; at this tiny data rate the extra link
  // margin matters far more than peak throughput in a room full of people.
  const uint8_t protocols = WIFI_PROTOCOL_11B | WIFI_PROTOCOL_11G |
                            WIFI_PROTOCOL_11N | WIFI_PROTOCOL_LR;
  if (esp_wifi_set_protocol(WIFI_IF_STA, protocols) != ESP_OK ||
      esp_wifi_set_channel(ESPNOW_CHANNEL, WIFI_SECOND_CHAN_NONE) != ESP_OK) {
    Serial.println("radio configuration failed");
    ESP.restart();
  }

  if (esp_now_init() != ESP_OK) {
    Serial.println("esp-now init failed");
    ESP.restart();
  }

  esp_now_peer_info_t peer = {};
  memcpy(peer.peer_addr, PEER, 6);
  peer.channel = ESPNOW_CHANNEL;
  peer.encrypt = false;
  if (esp_now_add_peer(&peer) != ESP_OK) {
    Serial.println("esp-now peer failed");
  }

  Serial.printf("transmitter up, %u buttons, mac %s\n", (unsigned)BUTTON_COUNT,
                WiFi.macAddress().c_str());
}

void loop() {
  debounce();
  update_leds();

  const uint32_t now = millis();
  if (now - last_send_ms < SEND_INTERVAL_MS) {
    return;
  }
  last_send_ms = now;

  PanelPacket packet = {PACKET_MAGIC, PACKET_VERSION, ++sequence, stable_mask};
  esp_now_send(PEER, reinterpret_cast<uint8_t *>(&packet), sizeof(packet));
}
