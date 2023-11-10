use std::fmt::Debug;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use rust_hooking_utils::raw_input::virtual_keys::VirtualKey;

pub const CONFIG_FILE_NAME: &str = "freecam_config.json";

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct FreecamConfig {
    /// Whether to open a console for logging
    pub console: bool,
    /// How often to run our simple update loop.
    pub update_rate: u16,
    /// If set, will allow the config to be reloaded during gameplay by providing the given key codes.
    pub reload_config_keys: Option<Vec<VirtualKey>>,
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

impl Default for FreecamConfig {
    fn default() -> Self {
        Self {
            console: false,
            update_rate: 144,
            reload_config_keys: Some(vec![VirtualKey::VK_CONTROL, VirtualKey::VK_SHIFT, VirtualKey::VK_R]),
            keybinds: Default::default(),
            camera: Default::default(),
            force_ttw_camera: true,
            block_game_middle_mouse_functionality: true,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct CameraConfig {
    pub custom_camera_enabled: bool,
    /// Whether camera rotation is inverted or not.
    pub inverted: bool,
    /// Whether the mouse scroll is inverted or not
    pub inverted_scroll: bool,
    /// Whether to adapt movement/scroll speed to be based on how far from the ground the camera is.
    ///
    /// Similar to the Warhammer TTW camera.
    pub ground_distance_speed: bool,
    pub sensitivity: f32,
    pub rotate_smoothing: f32,
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
            rotate_smoothing: 0.75,
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
    pub fast_key: VirtualKey,
    pub slow_key: VirtualKey,
    pub freecam_key: VirtualKey,
    pub forward_key: VirtualKey,
    pub backwards_key: VirtualKey,
    pub left_key: VirtualKey,
    pub right_key: VirtualKey,
    pub rotate_left: VirtualKey,
    pub rotate_right: VirtualKey,
}

impl Default for KeybindsConfig {
    fn default() -> Self {
        Self {
            fast_key: VirtualKey::VK_SHIFT,
            slow_key: VirtualKey::VK_MENU,
            freecam_key: VirtualKey::VK_MBUTTON,
            forward_key: VirtualKey::VK_W,
            backwards_key: VirtualKey::VK_S,
            left_key: VirtualKey::VK_A,
            right_key: VirtualKey::VK_D,
            rotate_left: VirtualKey::VK_Q,
            rotate_right: VirtualKey::VK_E,
        }
    }
}

pub fn load_config(directory: impl AsRef<Path>) -> anyhow::Result<FreecamConfig> {
    let path = directory.as_ref().join(CONFIG_FILE_NAME);
    let file = std::fs::read(&path)?;

    if let Ok(conf) = serde_json::from_slice(&file) {
        validate_config(&conf)?;
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

pub fn validate_config(conf: &FreecamConfig) -> anyhow::Result<()> {
    if (conf.camera.vertical_smoothing.abs() >= 1.) {
        anyhow::bail!(
            "Smoothening values should be in the range 0..1. Vertical smoothing was `{}`!",
            conf.camera.vertical_smoothing
        )
    }
    if (conf.camera.horizontal_smoothing.abs() >= 1.) {
        anyhow::bail!(
            "Smoothening values should be in the range 0..1. Horizontal smoothing was `{}`!",
            conf.camera.horizontal_smoothing
        )
    }
    if (conf.camera.rotate_smoothing.abs() >= 1.) {
        anyhow::bail!(
            "Smoothening values should be in the range 0..1. Rotate smoothing was `{}`!",
            conf.camera.rotate_smoothing
        )
    }
    if (conf.update_rate < 30) {
        anyhow::bail!("Update rate must be at least 30, was {}", conf.update_rate)
    }

    Ok(())
}
