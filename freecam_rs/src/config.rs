use std::fmt::Debug;
use std::path::Path;
use std::time::Duration;

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
    /// Any camera other than the `TotalWarCamera` (index 0) tends to bug out when going to a different unit.
    ///
    /// Forcing an override on every game start seems the most logical.
    pub force_ttw_camera: bool,
    /// Whether the base game's middle mouse functionality should be blocked during battles.
    ///
    /// Setting this to `true` allows the use of middle mouse button for the freecam.
    pub block_game_middle_mouse_functionality: bool,
    pub keybinds: KeybindsConfig,
    pub camera: CameraConfig,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct CameraConfig {
    pub custom_camera_enabled: bool,
    pub inverted: bool,
    pub inverted_scroll: bool,
    /// Whether to adapt movement/scroll speed to be based on how far from the ground the camera is.
    ///
    /// Similar to the Warhammer TTW camera.
    pub ground_distance_speed: bool,
    pub sensitivity: f32,
    pub pan_smoothing: f32,
    pub vertical_smoothing: f32,
    pub horizontal_smoothing: f32,
    pub horizontal_base_speed: f32,
    pub vertical_base_speed: f32,
    pub slow_multiplier: f32,
    pub fast_multiplier: f32,
    /// Whether to remain at a consistent height level above the terrain when moving the camera.
    pub maintain_relative_height: bool,
    pub relative_height_panning_delay: Duration,
    /// Whether to try to prevent the camera from clipping through the ground.
    pub prevent_ground_clipping: bool,
    /// How much of a difference there should _at least_ be between the ground level and the current camera position
    ///
    /// Setting this higher ensures less ground clipping will occur, but you won't be able to zoom in as much.
    pub ground_clip_margin: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            custom_camera_enabled: true,
            inverted: false,
            inverted_scroll: true,
            ground_distance_speed: true,
            sensitivity: 1.0,
            pan_smoothing: 0.75,
            vertical_smoothing: 0.92,
            horizontal_smoothing: 0.92,
            horizontal_base_speed: 1.0,
            vertical_base_speed: 1.0,
            fast_multiplier: 3.5,
            maintain_relative_height: true,
            slow_multiplier: 0.2,
            prevent_ground_clipping: true,
            ground_clip_margin: 1.3,
            relative_height_panning_delay: Duration::from_millis(25),
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
            freecam_key: 0x04,
            forward_key: 0x57,
            backwards_key: 0x53,
            left_key: 0x41,
            right_key: 0x44,
            rotate_left: 0x51,
            rotate_right: 0x45,
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
            camera: Default::default(),
            force_ttw_camera: true,
            block_game_middle_mouse_functionality: true,
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
