use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub enum BackupFrequency {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub gitea_url: Option<String>,
    pub gitea_repo: Option<String>,
    pub gitea_username: Option<String>,
    pub gitea_password: Option<String>,
    pub backup_paths: Vec<String>,
    pub last_backup: Option<String>,
    pub backup_name: Option<String>,
    pub backup_frequency: Option<BackupFrequency>,
    pub backup_time: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            return Ok(Config {
                gitea_url: None,
                gitea_repo: None,
                gitea_username: None,
                gitea_password: None,
                backup_paths: Vec::new(),
                last_backup: None,
                backup_name: None,
                backup_frequency: None,
                backup_time: None,
            });
        }

        let content = fs::read_to_string(config_path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::get_config_path()?;
        fs::create_dir_all(config_path.parent().unwrap())?;
        let content = serde_json::to_string_pretty(self)?;
        fs::write(config_path, content)?;
        Ok(())
    }

    fn get_config_path() -> io::Result<PathBuf> {
        Ok(dirs::home_dir()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "Could not find home directory")
            })?
            .join(".config")
            .join("obt")
            .join("config.json"))
    }
}
