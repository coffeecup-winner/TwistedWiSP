use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct TwistedWispConfigFormat {
    wisp: TwistedWispConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwistedWispConfig {
    pub executable_path: PathBuf,
    pub core_path: PathBuf,
    pub data_paths: Vec<PathBuf>,
    pub midi_in_port: Option<String>,
}

impl TwistedWispConfig {
    pub fn load_from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let config_text = std::fs::read_to_string(path)?;
        let config = toml::from_str::<TwistedWispConfigFormat>(&config_text)?;
        Ok(config.wisp)
    }
}
