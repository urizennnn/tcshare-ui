use rand::distr::Alphanumeric;
use std::any::type_name;
use std::{collections::BTreeMap, error::Error, fs};

use nanoid::nanoid;
use rand::{RngExt, rng};

use crate::config::app::app_config_variables::{App, Files};

pub fn check_config() -> Result<(), Box<dyn std::error::Error>> {
    log::info!("Checking config");

    let config_path = dirs::config_dir()
        .ok_or("Config directory not found")?
        .join("zync")
        .join("config.json");

    if !config_path.exists() {
        log::info!("File not found");
        return Err("Config not found".into());
    }

    log::info!("File found");
    Ok(())
}

pub fn create_config(key: &str) -> Result<(), Box<dyn Error>> {
    log::info!("Creating config");

    let client_id = nanoid!();
    let client_secret = create_secret(16);

    let config_path = dirs::config_dir()
        .ok_or("Config directory not found")?
        .join("zync")
        .join("config.json");

    if config_path.exists() {
        log::info!("File already exists");
        return Err("Config already exists".into());
    }

    let app_config = App {
        name: "Zync".to_string(),
        version: "1.0.0".to_string(),
        key: key.to_string(),
        secret: client_secret,
        client_id: client_id.to_string(),
        summary: Files {
            data: BTreeMap::new(),
        },
    };

    let serialized_config = serde_json::to_string_pretty(&app_config)?;

    if let Some(parent_dir) = config_path.parent() {
        fs::create_dir_all(parent_dir)?;
    }

    log::info!("Creating config file at {:?}", config_path);

    fs::write(config_path, serialized_config)?;

    log::info!("Config file created successfully");
    Ok(())
}
fn create_secret(len: usize) -> String {
    let client_secret = rng();
    client_secret
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
pub fn get_type<T>(_: &T)
where
    T: std::fmt::Debug,
{
    log::info!("Type: {}", type_name::<T>());
}
