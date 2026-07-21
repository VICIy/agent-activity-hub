use std::{io::Write, sync::Mutex, time::Duration};

use serde::Serialize;
use serialport::{SerialPort, SerialPortType};

use crate::led::{LedEffect, LedMapping};

const BAUD_RATE: u32 = 115_200;

#[derive(Clone, Debug, Serialize)]
pub struct Esp32Port {
    pub name: String,
    pub label: String,
    pub likely_esp32: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Esp32Status {
    pub connected: bool,
    pub port: Option<String>,
    pub error: Option<String>,
}

struct Connection {
    name: String,
    port: Box<dyn SerialPort>,
}

#[derive(Default)]
pub struct Esp32Manager {
    connection: Mutex<Option<Connection>>,
    last_error: Mutex<Option<String>>,
}

pub fn available_ports() -> Vec<Esp32Port> {
    let mut ports = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|port| {
            let (detail, likely_esp32) = match port.port_type {
                SerialPortType::UsbPort(info) => {
                    let product = info.product.unwrap_or_default();
                    let manufacturer = info.manufacturer.unwrap_or_default();
                    let text = format!("{manufacturer} {product}").trim().to_string();
                    let lower = text.to_ascii_lowercase();
                    (
                        text,
                        lower.contains("esp32")
                            || lower.contains("espressif")
                            || info.vid == 0x303a,
                    )
                }
                _ => (String::new(), false),
            };
            let label = if detail.is_empty() {
                port.port_name.clone()
            } else {
                format!("{} — {detail}", port.port_name)
            };
            Esp32Port {
                name: port.port_name,
                label,
                likely_esp32,
            }
        })
        .collect::<Vec<_>>();
    ports.sort_by_key(|port| (!port.likely_esp32, port.name.clone()));
    ports
}

impl Esp32Manager {
    pub fn connect(&self, name: &str) -> Result<(), String> {
        if !available_ports().iter().any(|port| port.name == name) {
            return Err(format!("serial port is unavailable: {name}"));
        }
        let mut port = serialport::new(name, BAUD_RATE)
            .timeout(Duration::from_millis(500))
            .open()
            .map_err(|error| format!("cannot open {name}: {error}"))?;
        port.write_all(b"{\"type\":\"hello\",\"protocol\":1}\n")
            .map_err(|error| format!("cannot initialize {name}: {error}"))?;
        *self
            .connection
            .lock()
            .expect("esp32 connection lock poisoned") = Some(Connection {
            name: name.into(),
            port,
        });
        *self.last_error.lock().expect("esp32 error lock poisoned") = None;
        Ok(())
    }

    pub fn disconnect(&self) {
        *self
            .connection
            .lock()
            .expect("esp32 connection lock poisoned") = None;
        *self.last_error.lock().expect("esp32 error lock poisoned") = None;
    }

    pub fn status(&self) -> Esp32Status {
        let connection = self
            .connection
            .lock()
            .expect("esp32 connection lock poisoned");
        Esp32Status {
            connected: connection.is_some(),
            port: connection.as_ref().map(|value| value.name.clone()),
            error: self
                .last_error
                .lock()
                .expect("esp32 error lock poisoned")
                .clone(),
        }
    }

    pub fn sync(&self, status: &str, mapping: &LedMapping, brightness: u8) {
        let effect = mapping
            .effects
            .get(status)
            .cloned()
            .unwrap_or(LedEffect::Solid { leds: "000".into() });
        let (leds, blink, period) = match effect {
            LedEffect::Solid { leds } => (leds, false, 500),
            LedEffect::Pattern { effect } => {
                (effect.mask, effect.pattern == "blink", effect.period)
            }
        };
        let payload = serde_json::json!({
            "type": "state", "protocol": 1, "status": status, "leds": leds,
            "blink": blink, "period": period, "brightness": brightness,
        });
        let mut bytes = payload.to_string().into_bytes();
        bytes.push(b'\n');
        let mut connection = self
            .connection
            .lock()
            .expect("esp32 connection lock poisoned");
        let write_error = connection
            .as_mut()
            .and_then(|value| value.port.write_all(&bytes).err());
        if let Some(error) = write_error {
            *connection = None;
            *self.last_error.lock().expect("esp32 error lock poisoned") = Some(error.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnected_manager_is_safe_to_sync() {
        let manager = Esp32Manager::default();
        manager.sync("working", &LedMapping::defaults(), 100);
        assert!(!manager.status().connected);
    }
}
