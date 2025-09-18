use std::error::Error;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub enable_anti_wobble: bool,
    pub enable_unblocked_guns: bool,
    pub enable_arcade_zoom: bool,
    pub max_zoom_level: u8,
    pub min_zoom_level: u8,
    pub zoom_levels: Vec<f32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enable_anti_wobble: false,
            enable_unblocked_guns: false,
            enable_arcade_zoom: true,
            max_zoom_level: 5,
            min_zoom_level: 3,
            zoom_levels: vec![14.0, 7.0, 1.0, 0.7, 0.5, 0.3],
        }
    }
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn Error>> {
        let config_str = std::fs::read_to_string(path)?;

        Ok(serde_json::from_str(&config_str)?)
    }

    pub fn save(&self, path: &str) -> Result<(), Box<dyn Error>> {
        let config_str = serde_json::to_string_pretty(self)?;

        std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap())?;
        std::fs::write(path, config_str)?;

        Ok(())
    }
}
