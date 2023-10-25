use std::fmt::Debug;
use std::path::Path;

use crate::ptr::NonNullPtr;
use anyhow::Context;
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
    pub addresses: AddressConfig,
}

/// All keys that need to be pressed for a speed state to be selected.
///
/// Expects [virtual key codes](https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes).
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct KeybindsConfig {
    pub pause_key: u16,
    pub exit_key: u16,
    pub fast_key: u16,
    pub slow_key: u16,
    pub freecam_key: u16,
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

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AddressConfig {
    pub battle_pointer: NonNullPtr<u32>,
    pub target_x: NonNullPtr<f32>,
    pub target_y: NonNullPtr<f32>,
    pub target_z: NonNullPtr<f32>,
    pub camera_x: NonNullPtr<f32>,
    pub camera_y: NonNullPtr<f32>,
    pub camera_z: NonNullPtr<f32>,
}

impl Default for AddressConfig {
    fn default() -> Self {
        Self {
            battle_pointer: 0x0193D683.into(),
            target_x: 0x0193D5DC.into(),
            target_y: 0x0193D5E4.into(),
            target_z: 0x0193D5E0.into(),
            camera_x: 0x0193D598.into(),
            camera_y: 0x0193D5A0.into(),
            camera_z: 0x0193D59C.into(),
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
            addresses: Default::default(),
        }
    }
}

pub fn load_config(directory: impl AsRef<Path>) -> anyhow::Result<FreecamConfig> {
    let file = std::fs::read(directory.as_ref().join(CONFIG_FILE_NAME))?;
    let conf = serde_json::from_slice(&file).context("Failed to read config file, is it valid?")?;

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
