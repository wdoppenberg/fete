// Finds out which GPIO each of your buttons is wired to.
//
// Flash this to the button board, open the serial monitor, and press the
// buttons one at a time in the order you want them numbered. Each press prints
// the pin it happened on. At the end it prints a BUTTON_PINS line you can paste
// straight into include/panel_config.h.
//
// This exists because the wiring is the one thing no amount of care in the
// firmware can guess, and because flashing over the board's original program
// destroys the only other record of it.

#include <Arduino.h>

// Every GPIO on an S3 that is safe to watch as an input. Deliberately excludes
// 33-37 (octal PSRAM), 19/20 (native USB), and 0/3/45/46 (strapping pins).
static const int CANDIDATES[] = {1,  2,  4,  5,  6,  7,  8,  9,  10, 11, 12,
                                 13, 14, 15, 16, 17, 18, 21, 38, 39, 40, 41,
                                 42, 47, 48};
static const size_t CANDIDATE_COUNT = sizeof(CANDIDATES) / sizeof(CANDIDATES[0]);

// Room for more than you have, so a stray press does not end the run.
static const size_t MAX_FOUND = 32;

static int last[CANDIDATE_COUNT];
static uint32_t changed_at[CANDIDATE_COUNT];
static int found[MAX_FOUND];
static size_t found_count = 0;

static const uint32_t DEBOUNCE_MS = 25;

/// Index this pin was already assigned to, or -1 if it is new.
static int found_index(int pin) {
  for (size_t i = 0; i < found_count; ++i) {
    if (found[i] == pin) {
      return static_cast<int>(i);
    }
  }
  return -1;
}

static void print_table() {
  Serial.print("\nstatic const int BUTTON_PINS[BUTTON_COUNT] = {");
  for (size_t i = 0; i < found_count; ++i) {
    Serial.print(found[i]);
    if (i + 1 < found_count) {
      Serial.print(", ");
    }
  }
  Serial.println("};\n");
}

void setup() {
  Serial.begin(115200);
  delay(2000);  // Give the USB port time to enumerate before the first print.

  for (size_t i = 0; i < CANDIDATE_COUNT; ++i) {
    pinMode(CANDIDATES[i], INPUT_PULLUP);
    last[i] = digitalRead(CANDIDATES[i]);
    changed_at[i] = 0;
  }

  Serial.println("pin discovery");
  Serial.println("press each button once, in the order you want them numbered");
  Serial.println("send any character to print the table so far\n");
}

void loop() {
  const uint32_t now = millis();

  for (size_t i = 0; i < CANDIDATE_COUNT; ++i) {
    const int reading = digitalRead(CANDIDATES[i]);
    if (reading == last[i]) {
      continue;
    }
    if (now - changed_at[i] < DEBOUNCE_MS) {
      continue;
    }
    changed_at[i] = now;
    last[i] = reading;

    // Only report the press, not the release, so one push is one line.
    if (reading == LOW) {
      const int seen = found_index(CANDIDATES[i]);
      if (seen >= 0) {
        Serial.printf("GPIO %d again — that is button %d\n", CANDIDATES[i], seen);
      } else if (found_count < MAX_FOUND) {
        Serial.printf("button %u  ->  GPIO %d\n", (unsigned)found_count,
                      CANDIDATES[i]);
        found[found_count++] = CANDIDATES[i];
      }
    }
  }

  if (Serial.available()) {
    while (Serial.available()) {
      Serial.read();
    }
    print_table();
  }
}
