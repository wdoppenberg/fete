// The board on the laptop's USB: listens for the panel and writes lines.
//
// Output format is ../../crates/fete-input-panel/PROTOCOL.md:
//
//   P <mask-hex> <seq> <age-ms>
//
// The age field is the half of the link only this board can see. Frames keep
// coming whether or not the panel is talking, so the host can tell "receiver
// unplugged" from "panel out of range", and treats a stale panel as no buttons
// held rather than latching whatever was down when it vanished.

#include <Arduino.h>
#include <WiFi.h>
#include <esp_now.h>
#include <esp_wifi.h>

#include "panel_config.h"

// Reported when nothing has ever arrived. Far beyond any timeout the host
// applies, so a panel that has never spoken reads as absent rather than as one
// that spoke a moment ago.
static const uint32_t NEVER_HEARD_MS = 999999;

static volatile uint32_t latest_buttons = 0;
static volatile uint32_t latest_rx_ms = 0;
static volatile bool heard_anything = false;
static uint16_t out_sequence = 0;
static uint32_t last_send_ms = 0;

static void on_packet(const uint8_t *data, int len) {
  if (len != static_cast<int>(sizeof(PanelPacket))) {
    return;
  }
  PanelPacket packet;
  memcpy(&packet, data, sizeof(packet));
  if (packet.magic != PACKET_MAGIC || packet.version != PACKET_VERSION) {
    return;
  }
  latest_buttons = packet.buttons;
  latest_rx_ms = millis();
  heard_anything = true;
}

// The callback signature changed in Arduino-ESP32 3.x. Supporting both means
// this builds whichever version PlatformIO resolves.
#if ESP_ARDUINO_VERSION_MAJOR >= 3
static void on_recv(const esp_now_recv_info_t *, const uint8_t *data, int len) {
  on_packet(data, len);
}
#else
static void on_recv(const uint8_t *, const uint8_t *data, int len) {
  on_packet(data, len);
}
#endif

void setup() {
  Serial.begin(115200);

  WiFi.mode(WIFI_STA);
  WiFi.disconnect();
  esp_wifi_set_channel(ESPNOW_CHANNEL, WIFI_SECOND_CHAN_NONE);

  if (esp_now_init() != ESP_OK) {
    Serial.println("# esp-now init failed");
    ESP.restart();
  }
  esp_now_register_recv_cb(on_recv);

  // A comment line, which the host ignores. Handy when you open the port in a
  // terminal and want to know the board came up rather than sitting mute.
  Serial.printf("# receiver up, channel %u, mac %s\n", (unsigned)ESPNOW_CHANNEL,
                WiFi.macAddress().c_str());
}

void loop() {
  const uint32_t now = millis();
  if (now - last_send_ms < SEND_INTERVAL_MS) {
    return;
  }
  last_send_ms = now;

  const uint32_t age = heard_anything ? (now - latest_rx_ms) : NEVER_HEARD_MS;
  Serial.printf("P %08lx %u %lu\n", static_cast<unsigned long>(latest_buttons),
                static_cast<unsigned>(++out_sequence),
                static_cast<unsigned long>(age));
}
