use chrono::{Local, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SolarThemeInfo {
    pub is_day: bool,
    pub current_time: String,
    pub sunrise_time: String,
    pub sunset_time: String,
    pub recommended_theme: String, // "light" or "dark"
}

pub struct SolarManager;

impl SolarManager {
    pub fn get_solar_info() -> SolarThemeInfo {
        let now = Local::now();
        let hour = now.hour();
        let minute = now.minute();
        let current_minutes = hour * 60 + minute;

        // Default solar window: Sunrise at 06:30 (390 min), Sunset at 18:30 (1110 min)
        let sunrise_minutes = 6 * 60 + 30; // 06:30
        let sunset_minutes = 18 * 60 + 30; // 18:30

        let is_day = current_minutes >= sunrise_minutes && current_minutes < sunset_minutes;

        SolarThemeInfo {
            is_day,
            current_time: format!("{:02}:{:02}", hour, minute),
            sunrise_time: "06:30".to_string(),
            sunset_time: "18:30".to_string(),
            recommended_theme: if is_day { "light".to_string() } else { "dark".to_string() },
        }
    }
}
