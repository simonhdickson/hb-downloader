use std::collections::{HashMap, HashSet};

use config::{Config, ConfigError, File, FileFormat};
use serde::{self, Deserialize};

#[derive(Clone, Debug, Deserialize)]
pub struct Settings {
    pub headers: HashMap<String, String>,
    pub platforms: HashSet<String>,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(File::from_str(
                include_str!("../config/default.toml"),
                FileFormat::Toml,
            ))
            .add_source(File::with_name("config").required(false))
            .build()?;

        config.try_deserialize()
    }
}
