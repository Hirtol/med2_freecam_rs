use anyhow::Context;
use std::path::Path;
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_R, VK_SHIFT};

pub const CONFIG_FILE_NAME: &str = "freecam_config.json";

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct FreecamConfig {
    /// Whether to open a console for logging
    pub console: bool,
    /// How often to run our simple update loop.
    pub update_rate: u16,
    /// If set, will allow the config to be reloaded during gameplay by providing the given key codes.
    pub reload_config_keys: Option<Vec<u16>>,
    pub keybinds: KeybindsConfig,
}

/// All keys that need to be pressed for a speed state to be selected.
///
/// Expects [virtual key codes](https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes).
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct KeybindsConfig {
    pause_key: u16,
    exit_key: u16,
    fast_key: u16,
    slow_key: u16,
    freecam_key: u16,
}

impl Default for KeybindsConfig {
    fn default() -> Self {
        Self {
            pause_key: 0x2D,
            exit_key: 0x23,
            fast_key: 0x10,
            slow_key: 0x12,
            freecam_key: 0x06,
        }
    }
}

impl Default for FreecamConfig {
    fn default() -> Self {
        Self {
            console: false,
            update_rate: 144,
            reload_config_keys: Some(vec![VK_CONTROL.0, VK_SHIFT.0, VK_R.0]),
            keybinds: Default::default(),
        }
    }
}

pub fn load_config(directory: impl AsRef<Path>) -> anyhow::Result<FreecamConfig> {
    let file = std::fs::read(directory.as_ref().join(CONFIG_FILE_NAME))?;
    let conf = serde_json::from_slice(&file).context("Failed to read config file, is it valid?")?;

    validate_config(&conf)?;

    Ok(conf)
}

pub fn create_initial_config(directory: impl AsRef<Path>) -> anyhow::Result<()> {
    let default_conf = FreecamConfig::default();
    let path = directory.as_ref().join(CONFIG_FILE_NAME);

    if !path.exists() {
        let mut file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(&mut file, &default_conf)?;
    }

    Ok(())
}

fn validate_config(config: &FreecamConfig) -> anyhow::Result<()> {
    let mut errors = Vec::new();

    let error = errors.join("\n");

    if error.is_empty() {
        Ok(())
    } else {
        Err(anyhow::Error::msg(error))
    }
}
