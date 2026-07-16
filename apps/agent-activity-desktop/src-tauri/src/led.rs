use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const MAPPING_KEY: &str = "led.mapping";
pub const BRIGHTNESS_KEY: &str = "led.brightness";
pub const PERIOD_DEFAULT_MIGRATION_KEY: &str = "led.period-default-v3";
pub const DEFAULT_PERIOD_MS: u32 = 500;
pub const MIN_BRIGHTNESS: u8 = 10;
pub const MAX_BRIGHTNESS: u8 = 100;
pub const DEFAULT_BRIGHTNESS: u8 = 100;

/// LED effect stored in settings. Mask positions are green, yellow, then red.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum LedEffect {
    Solid { leds: String },
    Pattern { effect: PatternEffect },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatternEffect {
    pub pattern: String,
    pub mask: String,
    pub period: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedMapping {
    pub effects: HashMap<String, LedEffect>,
}

impl LedMapping {
    pub fn defaults() -> Self {
        let mut effects = HashMap::new();
        effects.insert("offline".into(), LedEffect::Solid { leds: "000".into() });
        effects.insert("idle".into(), LedEffect::Solid { leds: "000".into() });
        effects.insert("working".into(), LedEffect::Solid { leds: "100".into() });
        effects.insert(
            "waiting_approval".into(),
            LedEffect::Pattern {
                effect: PatternEffect {
                    pattern: "blink".into(),
                    mask: "010".into(),
                    period: DEFAULT_PERIOD_MS,
                },
            },
        );
        effects.insert(
            "complete".into(),
            LedEffect::Pattern {
                effect: PatternEffect {
                    pattern: "blink".into(),
                    mask: "100".into(),
                    period: DEFAULT_PERIOD_MS,
                },
            },
        );
        effects.insert(
            "error".into(),
            LedEffect::Pattern {
                effect: PatternEffect {
                    pattern: "blink".into(),
                    mask: "001".into(),
                    period: DEFAULT_PERIOD_MS,
                },
            },
        );
        effects.insert("sleeping".into(), LedEffect::Solid { leds: "000".into() });
        Self { effects }
    }

    pub fn migrate_default_blink_periods(&mut self) -> bool {
        let mut changed = false;
        for status in ["waiting_approval", "complete", "error"] {
            let Some(LedEffect::Pattern { effect }) = self.effects.get_mut(status) else {
                continue;
            };
            if effect.pattern == "blink" && effect.period != DEFAULT_PERIOD_MS {
                effect.period = DEFAULT_PERIOD_MS;
                changed = true;
            }
        }
        changed
    }
}

pub fn clamp_brightness(brightness: u8) -> u8 {
    brightness.clamp(MIN_BRIGHTNESS, MAX_BRIGHTNESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_desktop_traffic_light_lifecycle() {
        let mapping = LedMapping::defaults();
        assert_eq!(
            mapping.effects.get("idle"),
            Some(&LedEffect::Solid { leds: "000".into() })
        );
        assert_eq!(
            mapping.effects.get("working"),
            Some(&LedEffect::Solid { leds: "100".into() })
        );
        for status in ["waiting_approval", "complete", "error"] {
            let LedEffect::Pattern { effect } = mapping.effects.get(status).unwrap() else {
                panic!("{status} should blink");
            };
            assert_eq!(effect.period, DEFAULT_PERIOD_MS);
        }
    }

    #[test]
    fn brightness_is_kept_within_the_supported_range() {
        assert_eq!(clamp_brightness(0), MIN_BRIGHTNESS);
        assert_eq!(clamp_brightness(55), 55);
        assert_eq!(clamp_brightness(255), MAX_BRIGHTNESS);
    }

    #[test]
    fn migrates_saved_default_blink_periods_once() {
        let mut mapping = LedMapping::defaults();
        mapping.effects.insert(
            "complete".into(),
            LedEffect::Pattern {
                effect: PatternEffect {
                    pattern: "blink".into(),
                    mask: "100".into(),
                    period: 800,
                },
            },
        );
        for status in ["waiting_approval", "complete", "error"] {
            let LedEffect::Pattern { effect } = mapping.effects.get_mut(status).unwrap() else {
                unreachable!();
            };
            effect.period = 800;
        }
        assert!(mapping.migrate_default_blink_periods());
        assert!(!mapping.migrate_default_blink_periods());
    }
}
