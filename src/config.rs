use std::{fs, path::Path};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    pub device: String,
    pub trigger: input_linux::Key,
    pub output_default: String,
    pub output_switched: String,
    #[serde(default)]
    pub grab: bool,
    #[serde(default)]
    pub debug: bool,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> crate::Result<Self> {
        let config = fs::read_to_string(&path)?;
        Ok(toml::from_str(&config)?)
    }
}
