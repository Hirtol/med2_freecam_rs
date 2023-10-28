use std::fmt::Debug;
use std::path::Path;

use anyhow::Context;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_R, VK_SHIFT};

use crate::data::{BattleCameraTargetView, BattleCameraType, BattleCameraView};
use crate::patch_locations::PATCH_LOCATIONS_STEAM;
use crate::ptr::NonNullPtr;

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
    pub camera: CameraConfig,
    /// Any camera other than the `TotalWarCamera` (index 0) tends to bug out when going to a different unit.
    ///
    /// Forcing an override on every game start seems the most logical.
    pub force_ttw_camera: bool,
    pub patch_locations: Vec<NonNullPtr<u8>>,
}

// #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, PartialOrd)]
// pub struct PatchLocation {
//     pub address: NonNullPtr<u8>,
// }

// impl PatchLocation {
//     pub fn new(address: usize) -> Self {
//         PatchLocation {
//             address: address.into(),
//         }
//     }
// }

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct CameraConfig {
    pub custom_camera_enabled: bool,
    pub inverted: bool,
    pub inverted_scroll: bool,
    pub sensitivity: f32,
    pub pan_smoothing: f32,
    pub vertical_smoothing: f32,
    pub horizontal_smoothing: f32,
    pub horizontal_base_speed: f32,
    pub vertical_base_speed: f32,
    pub slow_multiplier: f32,
    pub fast_multiplier: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            custom_camera_enabled: true,
            inverted: false,
            inverted_scroll: true,
            sensitivity: 1.0,
            pan_smoothing: 0.5,
            vertical_smoothing: 0.9,
            horizontal_smoothing: 0.9,
            horizontal_base_speed: 1.0,
            vertical_base_speed: 1.0,
            fast_multiplier: 3.5,
            slow_multiplier: 0.5,
        }
    }
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
    pub forward_key: u16,
    pub backwards_key: u16,
    pub left_key: u16,
    pub right_key: u16,
    pub rotate_left: u16,
    pub rotate_right: u16,
}

impl Default for KeybindsConfig {
    fn default() -> Self {
        Self {
            pause_key: 0x2D,
            exit_key: 0x23,
            fast_key: 0x10,
            slow_key: 0x12,
            freecam_key: 0x06,
            forward_key: 0x57,
            backwards_key: 0x53,
            left_key: 0x41,
            right_key: 0x44,
            rotate_left: 0x51,
            rotate_right: 0x45,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AddressConfig {
    pub battle_cam_conf_type: NonNullPtr<BattleCameraType>,
    /// Location which indicates whether a battle is currently on-going.
    ///
    /// If `!= 0` then it's `true`.
    pub battle_pointer: NonNullPtr<u32>,
    pub battle_cam_addr: NonNullPtr<BattleCameraView>,
    pub battle_cam_target_addr: NonNullPtr<BattleCameraTargetView>,
}

impl Default for AddressConfig {
    fn default() -> Self {
        Self {
            battle_cam_conf_type: 0x01639F14.into(),
            battle_pointer: 0x0193D683.into(),
            battle_cam_addr: 0x0193D598.into(),
            battle_cam_target_addr: 0x0193D5DC.into(),
        }
    }
}

impl Default for FreecamConfig {
    fn default() -> Self {
        Self {
            console: true,
            update_rate: 144,
            reload_config_keys: Some(vec![VK_CONTROL.0, VK_SHIFT.0, VK_R.0]),
            keybinds: Default::default(),
            addresses: Default::default(),
            camera: Default::default(),
            force_ttw_camera: true,
            patch_locations: PATCH_LOCATIONS_STEAM.into_iter().map(|loc| loc.into()).collect(),
        }
    }
}

pub fn load_config(directory: impl AsRef<Path>) -> anyhow::Result<FreecamConfig> {
    let path = directory.as_ref().join(CONFIG_FILE_NAME);
    let file = std::fs::read(&path)?;

    if let Ok(conf) = serde_json::from_slice(&file) {
        Ok(conf)
    } else {
        std::fs::remove_file(&path)?;
        create_initial_config(directory.as_ref())?;
        let file = std::fs::read(&path)?;
        serde_json::from_slice(&file).context("Couldn't load config.")
    }
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
