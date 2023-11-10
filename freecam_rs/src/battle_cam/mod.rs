use std::f32::consts::PI;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rust_hooking_utils::raw_input::key_manager::{KeyState, KeyboardManager};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, VIRTUAL_KEY, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

use data::Z_FIX_DELTA_GROUND_ADDR;
use data::{BattleCameraTargetView, BattleCameraType, BattleCameraView};

use crate::battle_cam::patches::{DynamicPatch, RemoteData};
use crate::config::FreecamConfig;
use crate::mouse::MouseManager;
use crate::patcher::LocalPatcher;

pub mod data;
pub mod patch_locations;
mod patches;

type Acceleration = Velocity;

#[derive(Default, Debug, Clone)]
pub struct Velocity {
    x: f32,
    y: f32,
    z: f32,
    pitch: f32,
    yaw: f32,
}

#[derive(Default, Debug)]
struct CustomCameraState {
    x: f32,
    y: f32,
    z: f32,
    pitch: f32,
    yaw: f32,
}

pub struct BattleCamera {
    current_state: BattleCameraState,
    patcher: LocalPatcher,
}

pub enum BattleCameraState {
    OutsideBattle,
    InBattle(BattleState),
}

impl BattleCamera {
    pub fn new(patcher: LocalPatcher) -> Self {
        Self {
            current_state: BattleCameraState::OutsideBattle,
            patcher,
        }
    }

    pub unsafe fn run(
        &mut self,
        conf: &mut FreecamConfig,
        scroll: &mut MouseManager,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
    ) -> anyhow::Result<()> {
        let in_battle = self.is_in_battle();

        // Handle state transitions
        match self.current_state {
            BattleCameraState::OutsideBattle if in_battle => {
                // Reset any scroll delta just to be sure.
                scroll.reset_scroll();
                self.current_state = BattleCameraState::InBattle(BattleState::new());
                Ok(())
            }
            BattleCameraState::InBattle(ref mut state) if in_battle => state.run(scroll, key_man, t_delta, conf),
            BattleCameraState::InBattle(_) if !in_battle => {
                // Transition out of battle, drop implementations take care of cleanup
                self.current_state = BattleCameraState::OutsideBattle;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Set whether the custom camera is currently enabled or not.
    ///
    /// Only really useful for config updates.
    pub fn set_custom_camera(&mut self, enabled: bool) {
        match &mut self.current_state {
            BattleCameraState::OutsideBattle => {}
            BattleCameraState::InBattle(b_state) => unsafe { b_state.change_camera_state(enabled) },
        }
    }

    pub fn is_in_battle(&self) -> bool {
        unsafe { *self.patcher.read(data::BATTLE_ONGOING_ADDR) != 0 }
    }
}

pub struct BattleState {
    battle_patcher: BattlePatcher,

    /// General data structure containing all data that is written to by the game thread.
    ///
    /// Note that this _must_ be below `battle_patcher` in the struct declaration to ensure the patches are removed
    /// before dropping this remote data.
    remote_data: RemoteData,
    custom_camera: CustomCameraState,
    velocity: Velocity,
    /// For panning
    last_sync_time: Option<Instant>,
    last_cursor_pos_freecam: Option<POINT>,
    /// The amount that our scroll differs from Z. Should help the camera remain consistent across terrain.
    z_diff: f32,
}

impl BattleState {
    /// Create a new ephemeral [BattleState] instance.
    ///
    /// A new struct should be created for each new battle.
    pub fn new() -> Self {
        let remote = RemoteData::default();

        Self {
            battle_patcher: BattlePatcher::new(&remote),
            velocity: Default::default(),
            custom_camera: Default::default(),
            z_diff: 0.0,
            remote_data: remote,
            last_cursor_pos_freecam: Default::default(),
            last_sync_time: None,
        }
    }

    pub unsafe fn change_camera_state(&mut self, enabled: bool) {
        if !enabled {
            self.battle_patcher.change_state(BattlePatchState::NotApplied);
        }
    }

    pub unsafe fn run(
        &mut self,
        scroll: &mut MouseManager,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        if conf.force_ttw_camera {
            // Always ensure we're on the TotalWar cam
            self.battle_patcher
                .patcher
                .write(data::BATTLE_CAM_CONF_TYPE_ADDR, BattleCameraType::TotalWar);
        }

        if !conf.camera.custom_camera_enabled {
            self.run_battle_no_custom(scroll, key_man, t_delta, conf)
        } else {
            self.run_battle_custom_camera(scroll, key_man, t_delta, conf)
        }
    }

    pub unsafe fn run_battle_no_custom(
        &mut self,
        mouse_man: &mut MouseManager,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        let target_pos = self.get_game_target_camera();
        let camera_pos = self.get_game_camera();
        let mut acceleration = Acceleration::default();

        let (mut pitch, mut yaw) = calculate_pitch_yaw(camera_pos, target_pos);

        let mut point = POINT::default();
        GetCursorPos(&mut point)?;

        // Adjust based on free-cam movement
        self.bc_handle_panning(key_man, mouse_man, conf, &mut acceleration, point, false);

        // Adjust pitch and yaw
        self.velocity.pitch += acceleration.pitch;
        self.velocity.yaw += acceleration.yaw;
        pitch += self.velocity.pitch;
        yaw += self.velocity.yaw;

        self.velocity.pitch *= conf.camera.pan_smoothing;
        self.velocity.yaw *= conf.camera.pan_smoothing;

        // Write to the addresses
        write_pitch_yaw(camera_pos, target_pos, pitch, yaw);
        Ok(())
    }

    unsafe fn run_battle_custom_camera(
        &mut self,
        scroll: &mut MouseManager,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        let camera_pos = self.get_game_camera();
        let mut acceleration = Acceleration::default();
        let (horizontal_speed, vertical_speed) = calculate_speed_multipliers(conf, key_man);

        let mut point = POINT::default();
        GetCursorPos(&mut point)?;

        // If some external source modified it with our consent we should probably update our camera.
        // This can happen when the user double clicked on the map or a unit and started panning towards them.
        if (self.custom_camera.x - camera_pos.x_coord).abs() > f32::EPSILON
            || (self.custom_camera.y - camera_pos.y_coord).abs() > f32::EPSILON
            || (self.custom_camera.z - camera_pos.z_coord).abs() > f32::EPSILON
        {
            self.sync_custom_camera();
            // Track the last time we had to sync the data for use in a hack in `bc_restrict_coordinates`.
            self.last_sync_time = Some(Instant::now());
        }

        // Handle camera teleportation
        self.bc_handle_camera_teleport(camera_pos);

        // Handle scroll
        self.bc_handle_scroll(scroll, conf, vertical_speed);

        // Adjust based on free-cam movement
        self.bc_handle_panning(key_man, scroll, conf, &mut acceleration, point, true);

        // Camera movement
        self.bc_move_camera(key_man, conf, &mut acceleration);

        // Rotation controls
        self.bc_handle_rotation(key_man, conf, &mut acceleration);

        // Update velocity based on the new `acceleration`
        self.velocity =
            Self::bc_calculate_next_velocity(conf, &self.velocity, &acceleration, horizontal_speed, vertical_speed);

        self.custom_camera.x += self.velocity.x;
        self.custom_camera.y += self.velocity.y;
        self.custom_camera.z += self.velocity.z;
        self.custom_camera.pitch += self.velocity.pitch;
        self.custom_camera.yaw += self.velocity.yaw;

        Self::bc_smooth_decay_velocity(&mut self.velocity, conf);

        self.bc_restrict_coordinates(&acceleration, conf);

        if matches!(self.battle_patcher.state, BattlePatchState::Applied) {
            self.write_full_custom_cam(camera_pos);
        } else {
            // Update our custom camera values.
            self.sync_custom_camera();
        }

        Ok(())
    }

    /// Handle the case where a user double clicks a unit card, and then presses a movement key to instantly teleport the
    /// camera toward the given unit.
    unsafe fn bc_handle_camera_teleport(&mut self, camera_pos: &mut BattleCameraView) {
        let teleport_location = self.remote_data.teleport_location.as_mut();
        // Check if all are different (in case of mid-write check).
        if teleport_location.is_available() {
            log::info!("Teleporting camera to: {:#?}", teleport_location);
            self.custom_camera.x = teleport_location.x;
            self.custom_camera.y = teleport_location.y;
            self.custom_camera.z = teleport_location.z;

            let target_pos = BattleCameraTargetView {
                x_coord: teleport_location.x_target,
                z_coord: teleport_location.z_target,
                y_coord: teleport_location.y_target,
            };
            let view_struct = BattleCameraView {
                x_coord: teleport_location.x,
                z_coord: teleport_location.z,
                y_coord: teleport_location.y,
            };
            let (pitch, yaw) = calculate_pitch_yaw(&view_struct, &target_pos);
            self.custom_camera.pitch = pitch;
            self.custom_camera.yaw = yaw;

            // Reset values.
            *teleport_location = Default::default();

            // Need to update the game height here manually or we risk a race condition where the `z_diff` will make
            // the camera jump up/down on the next frame.
            self.write_full_custom_cam(camera_pos);
            self.force_game_height_eval();
            // Update for maintaining relative height
            self.z_diff = self.custom_camera.z - self.get_ground_z_level();
        }
    }

    fn bc_handle_scroll(&mut self, scroll: &mut MouseManager, conf: &FreecamConfig, vertical_speed: f32) {
        // TODO: Figure out how this works.
        let scroll_delta = scroll.get_scroll_delta() * if conf.camera.inverted_scroll { -1 } else { 1 };
        let is_negative = if scroll_delta != 0 { scroll_delta.abs() / scroll_delta } else { 1 };
        self.velocity.z += (scroll_delta.pow(2) * is_negative) as f32 * vertical_speed / 10.;
    }

    unsafe fn bc_handle_panning(
        &mut self,
        key_man: &mut KeyboardManager,
        mouse_man: &mut MouseManager,
        conf: &mut FreecamConfig,
        acceleration: &mut Velocity,
        point: POINT,
        should_change_b_state: bool,
    ) {
        let state = key_man.get_key_state(VIRTUAL_KEY(conf.keybinds.freecam_key));
        match state {
            KeyState::Pressed => {
                let _ = GetCursorPos(self.last_cursor_pos_freecam.get_or_insert(POINT::default()));
                mouse_man.hide_cursor();
            }
            KeyState::Down => {
                if let Some(pos) = self.last_cursor_pos_freecam.as_ref() {
                    let invert = if conf.camera.inverted { -1.0 } else { 1.0 };
                    let adjusted_sens = conf.camera.sensitivity * (1. - conf.camera.pan_smoothing);
                    acceleration.pitch -= ((invert * (point.y - pos.y) as f32) / 500.) * adjusted_sens;
                    acceleration.yaw -= ((invert * (point.x - pos.x) as f32) / 500.) * adjusted_sens;

                    // Reset the cursor position to our set place.
                    let _ = SetCursorPos(pos.x, pos.y);

                    if should_change_b_state {
                        // We should have control again.
                        self.change_battle_state(false);
                    }
                }
            }
            KeyState::Released => {
                if let Some(pos) = self.last_cursor_pos_freecam.take() {
                    let _ = SetCursorPos(pos.x, pos.y);
                    mouse_man.show_cursor();
                }
            }
            KeyState::Up => {}
        }
    }

    unsafe fn bc_handle_rotation(
        &mut self,
        key_man: &mut KeyboardManager,
        conf: &mut FreecamConfig,
        acceleration: &mut Velocity,
    ) {
        let pan_speed = 1. - conf.camera.pan_smoothing;
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.rotate_left)) {
            acceleration.yaw += 0.03 * pan_speed;
            self.change_battle_state(false);
        }
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.rotate_right)) {
            acceleration.yaw -= 0.03 * pan_speed;
            self.change_battle_state(false);
        }
    }

    unsafe fn bc_move_camera(
        &mut self,
        key_man: &mut KeyboardManager,
        conf: &FreecamConfig,
        acceleration: &mut Velocity,
    ) {
        let yaw = self.custom_camera.yaw;
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.forward_key)) {
            acceleration.y += yaw.sin();
            acceleration.x += yaw.cos();
            self.change_battle_state(false);
        }
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.backwards_key)) {
            acceleration.y += (PI + yaw).sin();
            acceleration.x += (PI + yaw).cos();
            self.change_battle_state(false);
        }
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.left_key)) {
            acceleration.y += ((PI / 2.) + yaw).sin();
            acceleration.x += ((PI / 2.) + yaw).cos();
            self.change_battle_state(false);
        }
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.right_key)) {
            acceleration.y += ((3. * PI / 2.) + yaw).sin();
            acceleration.x += ((3. * PI / 2.) + yaw).cos();
            self.change_battle_state(false);
        }
    }

    fn bc_restrict_coordinates(&mut self, acceleration: &Acceleration, conf: &mut FreecamConfig) {
        self.custom_camera.x = 900.0f32.min((-900.0f32).max(self.custom_camera.x));
        self.custom_camera.y = 900.0f32.min((-900.0f32).max(self.custom_camera.y));
        self.custom_camera.z = 2400.0f32.min(self.custom_camera.z);

        // TODO: Add a new camera position struct which stores the _final_ value of a camera movement through scroll.
        // Then we can interpolate gradual movement between that state and the current camera position smoothly instead of jittery!

        // This `last_sync_time` is not a pretty check (and fragile for poorer performance PCs),
        // but it helps prevent buggy panning towards a particular point on the map (unit panning seems unaffected whether we have this or not).
        // The main benefit of this is that we can get rid of double click detection entirely. Hack for a hack...
        if conf.camera.maintain_relative_height
            && self
                .last_sync_time
                .as_ref()
                .map(|s| s.elapsed() > conf.camera.relative_height_panning_delay)
                .unwrap_or(true)
        {
            let new_z_diff = self.custom_camera.z - self.get_ground_z_level();

            if self.velocity.z.abs() > f32::EPSILON {
                self.z_diff = new_z_diff;
            } else if new_z_diff < self.z_diff {
                self.custom_camera.z += self.z_diff - new_z_diff;
            } else if new_z_diff > self.z_diff {
                self.custom_camera.z -= new_z_diff - self.z_diff;
            }

            // Can freely reset it now for a small performance improvement.
            self.last_sync_time = None;
        }

        // If we're below the ground we should probably move up!
        // This isn't a perfect solution, as one can still clip a bit, but floating a set amount above the ground kinda ruins the point.
        if conf.camera.prevent_ground_clipping {
            let z_bound = f32::from_bits(self.remote_data.remote_z.load(Ordering::SeqCst));
            let multiplier = if z_bound.is_sign_positive() { 1. } else { -1. };
            let clip_margin = multiplier * conf.camera.ground_clip_margin;

            if self.get_ground_z_level() != 0.
                && !z_bound.is_nan()
                && z_bound.is_finite()
                && ((self.custom_camera.z - self.get_ground_z_level()) < clip_margin)
            {
                self.custom_camera.z = (self.get_ground_z_level() + clip_margin).max(self.custom_camera.z);
            }

            // Force the game to re-evaluate the ground position relative to the camera and update its Z coordinate.
            // We need to do this as our velocity decays over time, during which the game is not updating its
            // Z coordinate because it's no longer receiving input! We need that in order to properly do the above.
            // We check for acceleration to ensure that we're not actively pressing buttons as an optimisation measure
            // (as the game would be calling the method itself anyway).
            if acceleration.y.abs() == 0.
                && acceleration.x.abs() == 0.
                && (self.velocity.x.abs() > f32::EPSILON || self.velocity.y.abs() > f32::EPSILON)
            {
                unsafe {
                    self.force_game_height_eval();
                }
            }
        }
    }

    unsafe fn force_game_height_eval(&mut self) {
        let remote_fn: unsafe extern "stdcall" fn(*mut f32, *mut f32, f32) =
            std::mem::transmute(data::CALCULATE_DELTA_Z_TO_GROUND_FN_ADDR);
        // As far as I can tell in Ghidra this uses up to an offset of 0x8 based on the base pointer, so 3 values.
        // (Specifically, it seems like a delta for the x, z, y coordinates respectively?)
        // Might be wrong, in which case, stack corruption yay!
        let mut delta_maybe = [0.0, 0.0, 0.0];
        // Also, yes, this is completely unsafe when it comes to thread safety.
        remote_fn(delta_maybe.as_mut_ptr(), Z_FIX_DELTA_GROUND_ADDR, 1.);
    }

    unsafe fn bc_calculate_next_velocity(
        conf: &FreecamConfig,
        current_velocity: &Velocity,
        acceleration: &Acceleration,
        horizontal_speed: f32,
        vertical_speed: f32,
    ) -> Velocity {
        let mut length = (acceleration.x.powi(2) + acceleration.y.powi(2) + acceleration.z.powi(2)).sqrt();

        if length == 0. {
            length = 1.;
        }

        Velocity {
            x: current_velocity.x
                + ((acceleration.x / length) * (horizontal_speed * (1. - conf.camera.horizontal_smoothing))) / 2.,
            y: current_velocity.y
                + ((acceleration.y / length) * (horizontal_speed * (1. - conf.camera.horizontal_smoothing))) / 2.,
            z: current_velocity.z
                + ((acceleration.z / length) * (vertical_speed * (1. - conf.camera.vertical_smoothing))) / 2.,
            pitch: current_velocity.pitch + acceleration.pitch,
            yaw: current_velocity.yaw + acceleration.yaw,
        }
    }

    fn bc_smooth_decay_velocity(velocity: &mut Velocity, conf: &FreecamConfig) {
        velocity.x *= conf.camera.horizontal_smoothing;
        velocity.y *= conf.camera.horizontal_smoothing;
        velocity.z *= conf.camera.vertical_smoothing;
        velocity.pitch *= conf.camera.pan_smoothing;
        velocity.yaw *= conf.camera.pan_smoothing;
    }

    unsafe fn change_battle_state(&mut self, paused: bool) {
        if paused {
            // No longer needed as we never set `paused` to true (and thus never need patches removed)
            // now that double click detection has been removed.
            // self.battle_patcher.change_state(BattlePatchState::SpecialOnlyApplied);
        } else {
            self.battle_patcher.change_state(BattlePatchState::Applied);
        }
    }

    unsafe fn sync_custom_camera(&mut self) {
        let target_pos = self.get_game_target_camera();
        let camera_pos = self.get_game_camera();

        let (pitch, yaw) = calculate_pitch_yaw(camera_pos, target_pos);

        self.custom_camera.x = camera_pos.x_coord;
        self.custom_camera.y = camera_pos.y_coord;
        self.custom_camera.z = camera_pos.z_coord;
        self.remote_data
            .remote_z
            .store(self.custom_camera.z.to_bits(), Ordering::SeqCst);
        self.custom_camera.pitch = pitch;
        self.custom_camera.yaw = yaw;
    }

    unsafe fn write_full_custom_cam(&mut self, camera_pos: &mut BattleCameraView) {
        // Important that this runs _before_ pitch/yaw adjustment as they're dependent.
        write_custom_camera(&self.custom_camera, camera_pos);

        let target_pos = self.get_game_target_camera();
        write_pitch_yaw(camera_pos, target_pos, self.custom_camera.pitch, self.custom_camera.yaw);
    }

    /// Return the current ground z-level
    ///
    /// We don't know the method/values directly, so we simply subtract the current [Z_FIX_DELTA_GROUND_ADDR] from the game's
    /// `remote_z` value.
    ///
    /// Note that this depends on the game's code updating these values. See [Self::force_game_height_eval] for forcing it.
    fn get_ground_z_level(&self) -> f32 {
        unsafe {
            f32::from_bits(self.remote_data.remote_z.load(Ordering::SeqCst))
                - *self.battle_patcher.patcher.read(Z_FIX_DELTA_GROUND_ADDR)
        }
    }

    unsafe fn get_game_camera<'a, 'b>(&'a self) -> &'b mut BattleCameraView {
        self.battle_patcher.patcher.mut_read(data::BATTLE_CAM_ADDR)
    }

    unsafe fn get_game_target_camera<'a, 'b>(&'a self) -> &'b mut BattleCameraTargetView {
        self.battle_patcher.patcher.mut_read(data::BATTLE_CAM_TARGET_ADDR)
    }
}

pub struct BattlePatcher {
    patcher: LocalPatcher,
    special_patcher: LocalPatcher,
    _dynamic_patches: Vec<DynamicPatch>,
    state: BattlePatchState,
}

pub enum BattlePatchState {
    /// All patches are applied and full camera control is taken away from the game
    Applied,
    /// Special patches which should _always_ be active whilst in battle are still applied, other camera patches are not applied.
    SpecialOnlyApplied,
    /// No patches are currently applied.
    NotApplied,
}

impl BattlePatcher {
    pub fn new(remote_data: &RemoteData) -> Self {
        let mut general_patcher = LocalPatcher::new();
        let mut special_patcher = LocalPatcher::new();

        // Always initialise our patcher with all the requisite patches.
        for patch in patch_locations::PATCH_LOCATIONS_STEAM {
            unsafe {
                patch_locations::patch_logic(patch, &mut general_patcher);
            }
        }

        patches::apply_general_z_remote_patch(&mut general_patcher, remote_data);
        // Special (dynamic) patches.
        let (teleport_patch, target_write_patch) = unsafe {
            let (teleport_patch, target_write_patch) =
                patches::create_unit_card_teleport_patch(remote_data.teleport_location.get_mut_ptr())
                    .expect("Failed to create teleport patch");
            teleport_patch.apply_to_patcher(&mut special_patcher);
            target_write_patch.apply_to_patcher(&mut special_patcher);

            (teleport_patch, target_write_patch)
        };

        Self {
            patcher: general_patcher,
            special_patcher,
            _dynamic_patches: vec![teleport_patch, target_write_patch],
            state: BattlePatchState::NotApplied,
        }
    }

    pub unsafe fn change_state(&mut self, new_state: BattlePatchState) {
        match self.state {
            BattlePatchState::Applied => match new_state {
                BattlePatchState::Applied => {}
                BattlePatchState::SpecialOnlyApplied => {
                    self.patcher.disable_all_patches();
                }
                BattlePatchState::NotApplied => {
                    self.patcher.disable_all_patches();
                    self.special_patcher.disable_all_patches();
                }
            },
            BattlePatchState::SpecialOnlyApplied => match new_state {
                BattlePatchState::Applied => {
                    self.patcher.enable_all_patches();
                }
                BattlePatchState::SpecialOnlyApplied => {}
                BattlePatchState::NotApplied => {
                    self.special_patcher.disable_all_patches();
                }
            },
            BattlePatchState::NotApplied => match new_state {
                BattlePatchState::Applied => {
                    self.patcher.enable_all_patches();
                    self.special_patcher.enable_all_patches();
                }
                BattlePatchState::SpecialOnlyApplied => {
                    self.special_patcher.enable_all_patches();
                }
                BattlePatchState::NotApplied => {}
            },
        }
        self.state = new_state;
    }
}

fn write_pitch_yaw(camera_pos: &BattleCameraView, target_pos: &mut BattleCameraTargetView, mut pitch: f32, yaw: f32) {
    pitch = pitch.max(-(PI / 2.) * 0.9);
    pitch = pitch.min((PI / 2.) * 0.9);

    target_pos.x_coord = (yaw.cos() * pitch.cos() * 1000.) + camera_pos.x_coord;
    target_pos.y_coord = (yaw.sin() * pitch.cos() * 1000.) + camera_pos.y_coord;
    target_pos.z_coord = (pitch.sin() * 1000.) + camera_pos.z_coord;
}

fn write_custom_camera(custom_cam: &CustomCameraState, camera_pos: &mut BattleCameraView) {
    camera_pos.x_coord = custom_cam.x;
    camera_pos.y_coord = custom_cam.y;
    camera_pos.z_coord = custom_cam.z;
}

fn calculate_pitch_yaw(camera_pos: &BattleCameraView, target_pos: &BattleCameraTargetView) -> (f32, f32) {
    let length = ((target_pos.x_coord - camera_pos.x_coord).powi(2)
        + (target_pos.y_coord - camera_pos.y_coord).powi(2)
        + (target_pos.z_coord - camera_pos.z_coord).powi(2))
    .sqrt();

    let mut pitch = ((target_pos.z_coord - camera_pos.z_coord) / length).asin();
    let mut yaw =
        ((target_pos.y_coord - camera_pos.y_coord) / length).atan2((target_pos.x_coord - camera_pos.x_coord) / length);

    if pitch.is_nan() {
        pitch = 0.;
    }
    if yaw.is_nan() {
        yaw = 0.;
    }

    (pitch, yaw)
}

fn calculate_speed_multipliers(conf: &FreecamConfig, key_man: &mut KeyboardManager) -> (f32, f32) {
    let has_fast = key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.fast_key));
    let has_slow = key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.slow_key));

    let multiplier = if has_fast {
        conf.camera.fast_multiplier
    } else if has_slow {
        conf.camera.slow_multiplier
    } else {
        1.0
    };

    (
        conf.camera.horizontal_base_speed * multiplier,
        conf.camera.vertical_base_speed * multiplier,
    )
}
