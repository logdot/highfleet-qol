use std::error::Error;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub enable_anti_wobble: bool,
    pub enable_arcade_zoom: bool,
    pub max_zoom_level: u8,
    pub min_zoom_level: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enable_anti_wobble: false,
            enable_arcade_zoom: true,
            max_zoom_level: 5,
            min_zoom_level: 3,
        }
    }
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn Error>> {
        let config_str = std::fs::read_to_string(path)?;

        Ok(serde_json::from_str(&config_str)?)
    }
}
