// Finds out how the button LEDs are wired.
//
// Discovery can find a button by watching a pin change, but nothing can find an
// output by watching it — so this app drives instead, and you watch the panel.
// Two wiring schemes account for almost every lit-button panel, and they need
// completely different firmware, so the first job is to tell them apart:
//
//   plain    one GPIO per LED, driven high (or low) to light it
//   chain    WS2812-style LEDs daisy-chained off a single data GPIO
//
// "Only the first button lights up" is the signature of a chain being fed one
// pixel's worth of data, or holding whatever the board's original program left
// in it, so a chain is the one to rule out first.
//
// Flash this, open the monitor, and follow the menu. Each step names the pin it
// is driving before it drives it, so the log tells you the answer; pressing any
// key during a sweep also flags whatever is under test at that moment, which is
// easier than reading a scrolling log with your eyes on the panel.
//
//   pio run -e led-discover -t upload && pio device monitor
//
// Nothing here touches BUTTON_PINS. Those are switch contacts to ground, and
// driving one high while somebody leans on the button is a dead short.

#include <Arduino.h>
#include <esp_cpu.h>
#include <soc/gpio_reg.h>

#include "panel_config.h"

// Every GPIO on an S3 that is safe to drive as an output, before the button
// pins are removed. Same exclusions as the `discover` app: 33-37 (octal PSRAM),
// 19/20 (native USB), 0/3/45/46 (strapping), 43/44 (UART0).
static const int CANDIDATES[] = {1,  2,  4,  5,  6,  7,  8,  9,  10, 11, 12,
                                 13, 14, 15, 16, 17, 18, 21, 38, 39, 40, 41,
                                 42, 47, 48};
static const size_t CANDIDATE_COUNT = sizeof(CANDIDATES) / sizeof(CANDIDATES[0]);

// CANDIDATES minus whatever panel_config.h says is a button, so the two tables
// cannot drift apart.
static int free_pins[CANDIDATE_COUNT];
static size_t free_count = 0;

// Pins the sweeps stay off until the ordinary ones have been exhausted, because
// each is doing another job at reset. As outputs after boot they are all fine:
//
//   43, 44   UART0 TX/RX. Nothing uses them here — this board's console is the
//            native USB CDC port — and TX *idles high*, which is a standing
//            invitation for a permanently lit LED.
//   0, 3     strapping. 0 held low at reset enters download mode, so it is
//            unusable as a button, but driving it once running costs nothing.
//   45, 46   strapping, sampled at reset only.
//
// 19 and 20 stay out even here: they are the native USB lines, and driving them
// takes the console down mid-walk, taking the answer with it.
static const int LAST_RESORT[] = {43, 44, 0, 3, 45, 46};
static const size_t LAST_RESORT_COUNT =
    sizeof(LAST_RESORT) / sizeof(LAST_RESORT[0]);

// How long each step of a sweep holds, in ms. Long enough to look up at the
// panel and back down at the log.
static const uint32_t STEP_MS = 1200;

// Pixels written during a chain test. Deliberately more than BUTTON_COUNT: a
// longer chain than you have costs nothing, and a shorter one would leave the
// far end of a real chain dark and looking like a plain-wired panel.
static const size_t CHAIN_PIXELS = 32;

// Per-pixel brightness for chain tests. 32 pixels at full white is over an amp;
// at this level the whole chain is comfortably inside what USB will give you,
// and it is still unmistakably lit in a dim room.
static const uint8_t CHAIN_LEVEL = 40;

static bool is_button_pin(int pin) {
  for (size_t i = 0; i < BUTTON_COUNT; ++i) {
    if (BUTTON_PINS[i] == pin) {
      return true;
    }
  }
  return false;
}

// ---------------------------------------------------------------------------
// Pin handling
// ---------------------------------------------------------------------------

// Every pin back to high impedance. Between steps this matters: a pin left
// driven low is a return path, and two LEDs lighting at once turns a clean
// answer into a puzzle.
static void release_all() {
  for (size_t i = 0; i < free_count; ++i) {
    pinMode(free_pins[i], INPUT);
  }
}

static void drive(int pin, int level) {
  pinMode(pin, OUTPUT);
  digitalWrite(pin, level);
}

// The set/clear registers for one pin. Bit-banging WS2812 timing cannot afford
// digitalWrite's function call and switch inside the 350 ns window.
struct PinRegs {
  volatile uint32_t *set;
  volatile uint32_t *clr;
  uint32_t mask;
};

static PinRegs pin_regs(int pin) {
  if (pin < 32) {
    return {reinterpret_cast<volatile uint32_t *>(GPIO_OUT_W1TS_REG),
            reinterpret_cast<volatile uint32_t *>(GPIO_OUT_W1TC_REG),
            1UL << pin};
  }
  return {reinterpret_cast<volatile uint32_t *>(GPIO_OUT1_W1TS_REG),
          reinterpret_cast<volatile uint32_t *>(GPIO_OUT1_W1TC_REG),
          1UL << (pin - 32)};
}

// ---------------------------------------------------------------------------
// WS2812
// ---------------------------------------------------------------------------

// One frame, bit-banged: 1.25 us per bit, with the high time carrying the
// value. Done from the cycle counter with interrupts off, because the protocol
// has no clock line and a scheduler tick in the middle of a bit corrupts the
// rest of the frame. 32 pixels is under a millisecond.
static portMUX_TYPE ws_mux = portMUX_INITIALIZER_UNLOCKED;

static void ws2812_write(int pin, const uint8_t *bytes, size_t len) {
  const PinRegs r = pin_regs(pin);
  const uint32_t mhz = getCpuFrequencyMhz();
  const uint32_t t0h = mhz * 350 / 1000;
  const uint32_t t1h = mhz * 700 / 1000;
  const uint32_t period = mhz * 1250 / 1000;

  pinMode(pin, OUTPUT);
  *r.clr = r.mask;
  delayMicroseconds(300);  // Latch: anything over ~50 us starts a new frame.

  taskENTER_CRITICAL(&ws_mux);
  uint32_t bit_start = esp_cpu_get_ccount();
  for (size_t i = 0; i < len; ++i) {
    for (int bit = 7; bit >= 0; --bit) {
      const uint32_t high = ((bytes[i] >> bit) & 1) ? t1h : t0h;
      while (esp_cpu_get_ccount() - bit_start < period) {
      }
      bit_start = esp_cpu_get_ccount();
      *r.set = r.mask;
      while (esp_cpu_get_ccount() - bit_start < high) {
      }
      *r.clr = r.mask;
    }
  }
  taskEXIT_CRITICAL(&ws_mux);
}

// Red, green and blue in turn down the chain. A single colour cannot tell a
// chain of ten from one stuck pixel; three rotating colours can be read off the
// panel from across the room, and the order tells you which end is pixel 0.
static void chain_frame(int pin) {
  static uint8_t frame[CHAIN_PIXELS * 3];
  for (size_t i = 0; i < CHAIN_PIXELS; ++i) {
    uint8_t *px = &frame[i * 3];  // WS2812 wants green first.
    px[0] = (i % 3) == 1 ? CHAIN_LEVEL : 0;
    px[1] = (i % 3) == 0 ? CHAIN_LEVEL : 0;
    px[2] = (i % 3) == 2 ? CHAIN_LEVEL : 0;
  }
  ws2812_write(pin, frame, sizeof(frame));
}

// ---------------------------------------------------------------------------
// Sweeps
// ---------------------------------------------------------------------------

// Hold for ms, returning true if a key arrived — the user flagging whatever is
// lit right now. Also true if they want out of a long sweep; both mean stop.
static bool hold(uint32_t ms) {
  const uint32_t until = millis() + ms;
  while (static_cast<int32_t>(millis() - until) < 0) {
    if (Serial.available()) {
      while (Serial.available()) {
        Serial.read();
      }
      return true;
    }
    delay(2);
  }
  return false;
}

static void flagged(const char *what) {
  Serial.printf("\n  >>> you flagged %s <<<\n\n", what);
}

// A pin's electrical signature, read without anyone watching the panel.
//
// Drive a pin high, then let go of it. A pin with nothing on it holds its charge
// for milliseconds — the pad is a few picofarads and the leakage is nanoamps. A
// pin with an LED and its resistor to ground collapses to zero the instant it is
// released. The same trick the other way up finds an LED wired to the supply.
//
// That is enough to separate the pins that have something hanging off them from
// the ones that do not, which is the slow half of discovery. Which button each
// one lights still needs eyes, but only over the handful this finds.
static const uint32_t SETTLE_US = 3000;

// Microseconds for a released pin to fall to LOW, or SETTLE_US if it never does.
static uint32_t decay_us(int pin, int from, int to) {
  drive(pin, from);
  delayMicroseconds(200);
  pinMode(pin, INPUT);

  const uint32_t start = micros();
  while (micros() - start < SETTLE_US) {
    if (digitalRead(pin) == to) {
      return micros() - start;
    }
  }
  return SETTLE_US;
}

static void probe_pins() {
  Serial.println("probe: what is hanging off each free pin\n");
  Serial.println("  pin  pullup  pulldn   fall     rise   verdict");

  for (size_t i = 0; i < free_count; ++i) {
    const int pin = free_pins[i];

    pinMode(pin, INPUT_PULLUP);
    delayMicroseconds(SETTLE_US);
    const int up = digitalRead(pin);

    pinMode(pin, INPUT_PULLDOWN);
    delayMicroseconds(SETTLE_US);
    const int down = digitalRead(pin);

    const uint32_t fall = decay_us(pin, HIGH, LOW);
    const uint32_t rise = decay_us(pin, LOW, HIGH);
    pinMode(pin, INPUT);

    // A pin that both falls and rises the moment it is released is held by
    // something on both sides — a resistor divider, or another driver. A pin
    // that only falls has a path to ground; only rises, a path to the supply.
    const bool held_low = fall < SETTLE_US;
    const bool held_high = rise < SETTLE_US;
    const char *verdict = "floats";
    if (held_low && held_high) {
      verdict = "driven or divided";
    } else if (held_low) {
      verdict = "LOAD to ground  <- LED candidate";
    } else if (held_high) {
      verdict = "load to supply";
    }

    Serial.printf("  %-3d  %-6s  %-6s  %5lu  %5lu   %s\n", pin,
                  up ? "high" : "low", down ? "high" : "low",
                  (unsigned long)fall, (unsigned long)rise, verdict);
  }

  release_all();
  Serial.println("\nfall/rise are microseconds, capped at 3000 = never moved.");
  Serial.println("run `d` over the candidates to see which button each lights.\n");
}

// Everything at once. Not a mapping, just an answer to "is any of this wired to
// a GPIO at all", which is worth thirty seconds before sweeping 15 pins twice.
static void sweep_all() {
  Serial.println("all free pins HIGH");
  for (size_t i = 0; i < free_count; ++i) {
    drive(free_pins[i], HIGH);
  }
  const bool hit_high = hold(STEP_MS * 3);
  release_all();
  if (hit_high) {
    flagged("the HIGH phase — LEDs are driven high, run `d`");
    return;
  }

  Serial.println("all free pins LOW");
  for (size_t i = 0; i < free_count; ++i) {
    drive(free_pins[i], LOW);
  }
  const bool hit_low = hold(STEP_MS * 3);
  release_all();
  if (hit_low) {
    flagged("the LOW phase — LEDs sink through the GPIO, LED_ON_LEVEL = LOW");
    return;
  }

  Serial.println("nothing lit. try `w` for a chain.");
}

// One pin at a time, both polarities, so a common-anode panel that only lights
// when the GPIO sinks is not read as a dead pin.
static void sweep_plain() {
  Serial.println("plain sweep: one pin at a time, HIGH then LOW");
  Serial.println("press any key the moment an LED lights\n");

  for (size_t i = 0; i < free_count; ++i) {
    const int pin = free_pins[i];

    Serial.printf("GPIO %-2d  HIGH\n", pin);
    drive(pin, HIGH);
    const bool hit_high = hold(STEP_MS);
    release_all();
    if (hit_high) {
      Serial.printf("  >>> GPIO %d, lights when driven HIGH <<<\n", pin);
      Serial.println("  put it in LED_PINS, keep LED_ON_LEVEL = HIGH\n");
      return;
    }

    Serial.printf("GPIO %-2d  LOW\n", pin);
    drive(pin, LOW);
    const bool hit_low = hold(STEP_MS);
    release_all();
    if (hit_low) {
      Serial.printf("  >>> GPIO %d, lights when driven LOW <<<\n", pin);
      Serial.println("  put it in LED_PINS and set LED_ON_LEVEL = LOW\n");
      return;
    }
  }

  Serial.println("\nswept every free pin, nothing flagged.");
  Serial.println("read the log for anything that lit, or try `w`.\n");
}

// A WS2812 frame on each pin in turn. A chain answers with a run of red, green
// and blue; a plain-wired panel answers with nothing, because the frame is a
// burst of sub-microsecond pulses and the eye averages it to dark.
static void sweep_chain() {
  Serial.println("chain sweep: a 32-pixel WS2812 frame on each pin");
  Serial.println("press any key the moment the panel lights\n");

  for (size_t i = 0; i < free_count; ++i) {
    const int pin = free_pins[i];
    Serial.printf("GPIO %-2d  chain frame\n", pin);

    // Repainted rather than written once: a chain latches and holds, so a pin
    // that already lit it stays lit while the next pin is tested, and the log
    // would name the wrong pin. Redrawing keeps the ownership obvious.
    const uint32_t until = millis() + STEP_MS;
    bool hit = false;
    while (static_cast<int32_t>(millis() - until) < 0 && !hit) {
      chain_frame(pin);
      hit = hold(50);
    }
    release_all();

    if (hit) {
      Serial.printf("\n  >>> GPIO %d drives the chain <<<\n", pin);
      Serial.println("  count the lit buttons and note which end is pixel 0\n");
      return;
    }
  }

  Serial.println("\nswept every free pin, no chain answered.");
  Serial.println("if `d` found nothing either, the LEDs are behind a shift");
  Serial.println("register or a matrix and the board has to be traced.\n");
}

// ---------------------------------------------------------------------------
// Mapping
// ---------------------------------------------------------------------------

// Which GPIO lights which button, without anyone writing anything down.
//
// The board lights one candidate pin and waits for a button to be pressed. The
// switch and the LED are different pins, so the press that answers "this one"
// also says which button it is, and the table falls out of the walk. Same shape
// as the `discover` app: press what it asks for, paste the line it prints.
// How long a pin blinks before the walk gives up on it. Short on purpose: five
// seconds is plenty to notice a blink you are already staring at, and a third of
// the free pins have no LED on them at all.
static const uint32_t MAP_TIMEOUT_MS = 5000;

// Half-period of the test blink. Fast enough to be obvious at a glance, slow
// enough that it is plainly a blink and not a flicker.
static const uint32_t BLINK_MS = 150;

static int led_for_button[BUTTON_COUNT];

// Index of a button that is down, or -1.
static int pressed_button() {
  for (size_t i = 0; i < BUTTON_COUNT; ++i) {
    if (digitalRead(BUTTON_PINS[i]) == BUTTON_PRESSED_LEVEL) {
      return static_cast<int>(i);
    }
  }
  return -1;
}

static void wait_all_released() {
  uint32_t quiet_since = millis();
  while (millis() - quiet_since < DEBOUNCE_MS) {
    if (pressed_button() >= 0) {
      quiet_since = millis();
    }
    delay(2);
  }
}

static void print_led_table() {
  Serial.print("\nstatic const int LED_PINS[BUTTON_COUNT] = {");
  for (size_t i = 0; i < BUTTON_COUNT; ++i) {
    Serial.print(led_for_button[i]);
    if (i + 1 < BUTTON_COUNT) {
      Serial.print(", ");
    }
  }
  Serial.println("};\n");
}

// True if some button already claims this pin, so a second run of the walk only
// covers what is still unknown.
static bool pin_claimed(int pin) {
  for (size_t i = 0; i < BUTTON_COUNT; ++i) {
    if (led_for_button[i] == pin) {
      return true;
    }
  }
  return false;
}

static void map_leds(const int *pins, size_t count) {
  Serial.println("mapping: one pin blinking at a time");
  Serial.println("press the button that CHANGES — it may light, blink, or go");
  Serial.println("dark if it was already lit. any key if nothing changes.\n");
  wait_all_released();

  for (size_t i = 0; i < count; ++i) {
    const int pin = pins[i];
    if (pin_claimed(pin)) {
      continue;  // Already answered, by this run or by the table it started from.
    }
    Serial.printf("GPIO %-2d  blinking\n", pin);

    // Blinked rather than held on. A panel can have a button that is lit all the
    // time from something else on the board, and against a steady test light
    // that button looks like the answer at every single step — which is exactly
    // how the first run of this came back with one button claiming eleven pins.
    // Nothing else on the panel blinks, so a blink is unambiguous.
    int button = -1;
    bool on = false;
    uint32_t next_toggle = 0;
    const uint32_t until = millis() + MAP_TIMEOUT_MS;
    while (static_cast<int32_t>(millis() - until) < 0) {
      if (static_cast<int32_t>(millis() - next_toggle) >= 0) {
        on = !on;
        drive(pin, on ? HIGH : LOW);
        next_toggle = millis() + BLINK_MS;
      }
      if (Serial.available()) {
        while (Serial.available()) {
          Serial.read();
        }
        break;
      }
      const int candidate = pressed_button();
      if (candidate >= 0) {
        delay(DEBOUNCE_MS);
        if (pressed_button() == candidate) {
          button = candidate;
          break;
        }
      }
      delay(2);
    }
    release_all();

    if (button < 0) {
      Serial.println("  nothing in 5 s — no LED on this pin");
      continue;
    }
    if (led_for_button[button] >= 0) {
      Serial.printf("  button %d was already GPIO %d; taking %d instead\n",
                    button, led_for_button[button], pin);
    }
    led_for_button[button] = pin;
    Serial.printf("  button %d  ->  GPIO %d\n", button, pin);
    wait_all_released();
  }

  Serial.println("\npaste this into include/panel_config.h:");
  print_led_table();
}

// ---------------------------------------------------------------------------

static void menu() {
  Serial.println("\nled discovery");
  Serial.printf("free pins (candidates minus the %u buttons): ",
                (unsigned)BUTTON_COUNT);
  for (size_t i = 0; i < free_count; ++i) {
    Serial.printf("%d%s", free_pins[i], i + 1 < free_count ? ", " : "\n");
  }
  Serial.println("  m  map pins to buttons — blink one, press the one blinking");
  Serial.println("  l  map over the last-resort pins: UART0 and the strappings");
  Serial.println("  x  forget the table, so `m` walks every pin again");
  Serial.println("  p  probe every free pin electrically, no eyes needed");
  Serial.println("  a  all free pins on together — is anything wired at all");
  Serial.println("  d  plain sweep, one GPIO per LED");
  Serial.println("  w  chain sweep, WS2812 on one data GPIO");
  Serial.println("  s  stop, everything back to high impedance");
  Serial.println("during a sweep, any key flags the step under test.\n");
}

void setup() {
  Serial.begin(115200);
  delay(2000);  // Give the USB port time to enumerate before the first print.

  for (size_t i = 0; i < CANDIDATE_COUNT; ++i) {
    if (!is_button_pin(CANDIDATES[i])) {
      free_pins[free_count++] = CANDIDATES[i];
    }
  }
  for (size_t i = 0; i < BUTTON_COUNT; ++i) {
    pinMode(BUTTON_PINS[i], INPUT_PULLUP);
    // Start from whatever panel_config.h already knows, so a second walk only
    // has to find the buttons the first one missed.
    led_for_button[i] = LED_PINS[i];
  }
  release_all();

  menu();
}

void loop() {
  if (!Serial.available()) {
    delay(10);
    return;
  }

  const int key = Serial.read();
  while (Serial.available()) {
    Serial.read();
  }

  switch (key) {
    case 'm':
      map_leds(free_pins, free_count);
      break;
    case 'l':
      map_leds(LAST_RESORT, LAST_RESORT_COUNT);
      break;
    case 'x':
      for (size_t i = 0; i < BUTTON_COUNT; ++i) {
        led_for_button[i] = -1;
      }
      Serial.println("table cleared; `m` now walks every free pin");
      break;
    case 'p':
      probe_pins();
      break;
    case 'a':
      sweep_all();
      break;
    case 'd':
      sweep_plain();
      break;
    case 'w':
      sweep_chain();
      break;
    case 's':
      release_all();
      Serial.println("all pins released");
      break;
    case '\r':
    case '\n':
      break;
    default:
      menu();
      break;
  }
}
