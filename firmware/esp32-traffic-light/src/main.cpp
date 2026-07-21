#include <Arduino.h>
#include <ArduinoJson.h>
#include <NimBLEDevice.h>

// GFlash6/minic traffic-light board: three common-anode LEDs. Override these
// in platformio.ini when using a differently wired ESP32 board.
#ifndef LED_COMMON_ANODE_PIN
#define LED_COMMON_ANODE_PIN 7
#endif
#ifndef LED_GREEN_PIN
#define LED_GREEN_PIN 10
#endif
#ifndef LED_YELLOW_PIN
#define LED_YELLOW_PIN 9
#endif
#ifndef LED_RED_PIN
#define LED_RED_PIN 8
#endif
#ifndef LED_ACTIVE_LOW
#define LED_ACTIVE_LOW 1
#endif

static constexpr char SERVICE_UUID[] = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
static constexpr char RX_UUID[] = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";
static constexpr char TX_UUID[] = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";

struct LightState {
  bool lamps[3] = {false, false, false}; // green, yellow, red
  bool blink = false;
  bool phaseOn = true;
  uint32_t period = 500;
  uint32_t changedAt = 0;
  uint8_t brightness = 100;
} state;

String serialLine;
String bleLine;
NimBLECharacteristic *txCharacteristic = nullptr;

void writeLamp(uint8_t pin, bool on) {
  uint8_t value = on ? map(state.brightness, 0, 100, 0, 255) : 0;
#if LED_ACTIVE_LOW
  value = 255 - value;
#endif
  analogWrite(pin, value);
}

void render() {
  const bool enabled = !state.blink || state.phaseOn;
  writeLamp(LED_GREEN_PIN, enabled && state.lamps[0]);
  writeLamp(LED_YELLOW_PIN, enabled && state.lamps[1]);
  writeLamp(LED_RED_PIN, enabled && state.lamps[2]);
}

void applyMessage(const String &line) {
  JsonDocument document;
  if (deserializeJson(document, line) != DeserializationError::Ok) return;
  if (String(document["type"] | "") == "hello") {
    if (txCharacteristic) {
      txCharacteristic->setValue("{\"type\":\"ready\",\"protocol\":1}\n");
      txCharacteristic->notify();
    }
    Serial.println("{\"type\":\"ready\",\"protocol\":1}");
    return;
  }
  if (String(document["type"] | "") != "state") return;

  String lamps = document["leds"] | "000";
  for (int index = 0; index < 3; index++) state.lamps[index] = lamps.length() > index && lamps[index] == '1';
  state.blink = document["blink"] | false;
  state.period = constrain(document["period"] | 500, 20, 10000);
  state.brightness = constrain(document["brightness"] | 100, 0, 100);
  state.phaseOn = true;
  state.changedAt = millis();
  render();
}

class RxCallbacks : public NimBLECharacteristicCallbacks {
  void onWrite(NimBLECharacteristic *characteristic, NimBLEConnInfo &) override {
    const std::string value = characteristic->getValue();
    for (const char byte : value) {
      if (byte == '\n') {
        applyMessage(bleLine);
        bleLine = "";
      } else if (bleLine.length() < 512) {
        bleLine += byte;
      }
    }
  }
};

void setupBle() {
  NimBLEDevice::init("Agent Activity Light");
  NimBLEServer *server = NimBLEDevice::createServer();
  NimBLEService *service = server->createService(SERVICE_UUID);
  NimBLECharacteristic *rx = service->createCharacteristic(RX_UUID, NIMBLE_PROPERTY::WRITE | NIMBLE_PROPERTY::WRITE_NR);
  txCharacteristic = service->createCharacteristic(TX_UUID, NIMBLE_PROPERTY::READ | NIMBLE_PROPERTY::NOTIFY);
  rx->setCallbacks(new RxCallbacks());
  service->start();
  NimBLEAdvertising *advertising = NimBLEDevice::getAdvertising();
  advertising->addServiceUUID(SERVICE_UUID);
  advertising->enableScanResponse(true);
  advertising->start();
}

void setup() {
#if LED_COMMON_ANODE_PIN >= 0
  pinMode(LED_COMMON_ANODE_PIN, OUTPUT);
  digitalWrite(LED_COMMON_ANODE_PIN, HIGH);
#endif
  pinMode(LED_GREEN_PIN, OUTPUT);
  pinMode(LED_YELLOW_PIN, OUTPUT);
  pinMode(LED_RED_PIN, OUTPUT);
  Serial.begin(115200);
  setupBle();
  render();
}

void loop() {
  while (Serial.available()) {
    const char byte = static_cast<char>(Serial.read());
    if (byte == '\n') {
      applyMessage(serialLine);
      serialLine = "";
    } else if (serialLine.length() < 512) {
      serialLine += byte;
    }
  }
  if (state.blink && millis() - state.changedAt >= state.period) {
    state.changedAt = millis();
    state.phaseOn = !state.phaseOn;
    render();
  }
  delay(2);
}
