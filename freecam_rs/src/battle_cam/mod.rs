use std::f32::consts::PI;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::battle_cam::patches::{DynamicPatch, RemoteData};
use rust_hooking_utils::raw_input::key_manager::{KeyState, KeyboardManager};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, VIRTUAL_KEY, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::config::FreecamConfig;
use crate::data::{BattleCameraTargetView, BattleCameraType, BattleCameraView};
use crate::mouse::ScrollTracker;
use crate::patcher::LocalPatcher;

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

#[derive(Default)]
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
        scroll: &mut ScrollTracker,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
    ) -> anyhow::Result<()> {
        let in_battle = *self.patcher.read(conf.addresses.battle_pointer.as_ref()) != 0;

        // Handle state transitions
        match self.current_state {
            BattleCameraState::OutsideBattle if in_battle => {
                self.current_state = BattleCameraState::InBattle(BattleState::new(conf));
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
    // Patch related state
    old_cursor_pos: POINT,
    last_left_click: Instant,
    is_moving_toward_unit: bool,
    /// The amount that our scroll differs from Z. Should help the camera remain consistent across terrain.
    z_diff: f32,
    /// The lowest bound for Z at a particular point in time to prevent the camera from sinking below the terrain.
    minimal_z: f32,
}

impl BattleState {
    /// Create a new ephemeral [BattleState] instance.
    ///
    /// A new struct should be created for each new battle.
    pub fn new(conf: &mut FreecamConfig) -> Self {
        let mut point = POINT::default();
        let _ = unsafe { GetCursorPos(&mut point) };
        let remote = RemoteData::default();

        Self {
            battle_patcher: BattlePatcher::new(conf, &remote),
            old_cursor_pos: point,
            velocity: Default::default(),
            last_left_click: Instant::now(),
            custom_camera: Default::default(),
            is_moving_toward_unit: false,
            z_diff: 0.0,
            minimal_z: 0.0,
            remote_data: remote,
        }
    }

    pub unsafe fn change_camera_state(&mut self, enabled: bool) {
        if !enabled {
            self.battle_patcher.change_state(BattlePatchState::NotApplied);
        }
    }

    pub unsafe fn run(
        &mut self,
        scroll: &mut ScrollTracker,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        if conf.force_ttw_camera {
            // Always ensure we're on the TotalWar cam
            self.battle_patcher
                .patcher
                .write(conf.addresses.battle_cam_conf_type.as_mut(), BattleCameraType::TotalWar);
        }

        println!("Remote data: {:#?}", self.remote_data);

        if !conf.camera.custom_camera_enabled {
            self.run_battle_no_custom(key_man, t_delta, conf)
        } else {
            self.run_battle_custom_camera(scroll, key_man, t_delta, conf)
        }
    }

    pub unsafe fn run_battle_no_custom(
        &mut self,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        let target_pos = self
            .battle_patcher
            .patcher
            .mut_read(conf.addresses.battle_cam_target_addr.as_mut());
        let camera_pos = self
            .battle_patcher
            .patcher
            .mut_read(conf.addresses.battle_cam_addr.as_mut());
        let mut acceleration = Acceleration::default();

        let (mut pitch, mut yaw) = calculate_length_pitch_yaw(camera_pos, target_pos);

        let mut point = POINT::default();
        GetCursorPos(&mut point)?;

        // Adjust based on free-cam movement
        self.bc_handle_panning(key_man, conf, &mut acceleration, point, false);

        // Adjust pitch and yaw
        self.velocity.pitch += acceleration.pitch;
        self.velocity.yaw += acceleration.yaw;
        pitch += self.velocity.pitch;
        yaw += self.velocity.yaw;

        self.velocity.pitch *= conf.camera.pan_smoothing;
        self.velocity.yaw *= conf.camera.pan_smoothing;

        // Write to the addresses
        write_pitch_yaw(camera_pos, target_pos, pitch, yaw);

        // Persist info for next loop
        self.old_cursor_pos = point;
        Ok(())
    }

    unsafe fn run_battle_custom_camera(
        &mut self,
        scroll: &mut ScrollTracker,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        let camera_pos = self
            .battle_patcher
            .patcher
            .mut_read(conf.addresses.battle_cam_addr.as_mut());
        let mut acceleration = Acceleration::default();
        let (horizontal_speed, vertical_speed) = calculate_speed_multipliers(conf, key_man);

        let mut point = POINT::default();
        GetCursorPos(&mut point)?;

        // If some external source modified it with our consent we should probably update our camera.
        if (self.custom_camera.x - camera_pos.x_coord).abs() > f32::EPSILON
            || (self.custom_camera.y - camera_pos.y_coord).abs() > f32::EPSILON
            || (self.custom_camera.z - camera_pos.z_coord).abs() > f32::EPSILON
        {
            self.sync_custom_camera(conf);
        }

        // Handle scroll
        self.bc_handle_scroll(scroll, conf, vertical_speed);

        // Detect double click (vanilla functionality retention)
        self.bc_handle_left_click(key_man, point);

        // Adjust based on free-cam movement
        self.bc_handle_panning(key_man, conf, &mut acceleration, point, true);

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

        self.bc_restrict_coordinates(conf);

        if matches!(self.battle_patcher.state, BattlePatchState::Applied) {
            println!("WRITING VALUES: {:#?}", self.velocity);
            // Important that this runs _before_ pitch/yaw adjustment as they're dependent.
            write_custom_camera(&self.custom_camera, camera_pos);

            let target_pos = self
                .battle_patcher
                .patcher
                .mut_read(conf.addresses.battle_cam_target_addr.as_mut());
            write_pitch_yaw(camera_pos, target_pos, self.custom_camera.pitch, self.custom_camera.yaw);
        } else {
            // Update our custom camera values.
            self.sync_custom_camera(conf);
        }

        // Persist info for next loop
        self.old_cursor_pos = point;
        Ok(())
    }

    unsafe fn bc_handle_left_click(&mut self, key_man: &mut KeyboardManager, point: POINT) {
        if key_man.get_key_state(VK_LBUTTON) == KeyState::Pressed {
            let now = Instant::now();
            let time_since_last = now.duration_since(self.last_left_click);
            self.last_left_click = now;

            if (time_since_last.as_millis() as u32) < GetDoubleClickTime()
                && (self.old_cursor_pos.x - point.x).abs() < 10
                && (self.old_cursor_pos.y - point.y).abs() < 10
            {
                self.is_moving_toward_unit = true;
                self.change_battle_state(true);
            }
        }
    }

    fn bc_handle_scroll(&mut self, scroll: &mut ScrollTracker, conf: &FreecamConfig, vertical_speed: f32) {
        // TODO: Figure out how this works.
        let scroll_delta = scroll.get_scroll_delta() * if conf.camera.inverted_scroll { -1 } else { 1 };
        let is_negative = if scroll_delta != 0 { scroll_delta.abs() / scroll_delta } else { 1 };
        self.velocity.z += (scroll_delta.pow(2) * is_negative) as f32 * vertical_speed / 10.;
    }

    unsafe fn bc_handle_panning(
        &mut self,
        key_man: &mut KeyboardManager,
        conf: &mut FreecamConfig,
        acceleration: &mut Velocity,
        point: POINT,
        should_change_b_state: bool,
    ) {
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.freecam_key)) {
            let invert = if conf.camera.inverted { -1.0 } else { 1.0 };
            let adjusted_sens = conf.camera.sensitivity * (1. - conf.camera.pan_smoothing);
            acceleration.pitch -= ((invert * (point.y - self.old_cursor_pos.y) as f32) / 500.) * adjusted_sens;
            acceleration.yaw -= ((invert * (point.x - self.old_cursor_pos.x) as f32) / 500.) * adjusted_sens;
            if should_change_b_state {
                // We should have control again.
                self.change_battle_state(false);
            }
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

    fn bc_restrict_coordinates(&mut self, conf: &mut FreecamConfig) {
        self.custom_camera.x = 900.0f32.min((-900.0f32).max(self.custom_camera.x));
        self.custom_camera.y = 900.0f32.min((-900.0f32).max(self.custom_camera.y));

        if conf.camera.maintain_relative_height {
            let new_z_diff = self.custom_camera.z - f32::from_bits(self.remote_data.remote_z.load(Ordering::SeqCst));

            if self.velocity.z.abs() > f32::EPSILON {
                self.z_diff = new_z_diff;
            } else if new_z_diff < self.z_diff {
                self.custom_camera.z += self.z_diff - new_z_diff;
            } else if new_z_diff > self.z_diff {
                self.custom_camera.z -= new_z_diff - self.z_diff;
            }
        }

        // If we're below the ground we should probably move up!
        // This isn't a perfect solution, as one can still clip a bit, but floating a set amount above the ground kinda ruins the point.
        if conf.camera.prevent_ground_clipping {
            let z_bound = f32::from_bits(self.remote_data.remote_z.load(Ordering::SeqCst));
            let mut still_changing = false;
            // Ensure there's still changes happening
            if (self.minimal_z - z_bound).abs() > f32::EPSILON {
                self.minimal_z = z_bound;
                still_changing = true;
            }

            let multiplier = if z_bound.is_sign_positive() { -1. } else { 1. };
            // !still_changing
            if self.minimal_z != 0.
                && !z_bound.is_nan()
                && z_bound.is_finite()
                && ((self.custom_camera.z - self.minimal_z) < (multiplier * 2.1))
            {
                self.custom_camera.z = (self.minimal_z + (multiplier * 2.1)).max(self.custom_camera.z);
            }
        }
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
            self.battle_patcher.change_state(BattlePatchState::SpecialOnlyApplied);
        } else {
            self.battle_patcher.change_state(BattlePatchState::Applied);
        }
    }

    unsafe fn sync_custom_camera(&mut self, conf: &mut FreecamConfig) {
        let target_pos = self
            .battle_patcher
            .patcher
            .mut_read(conf.addresses.battle_cam_target_addr.as_mut());
        let camera_pos = self
            .battle_patcher
            .patcher
            .mut_read(conf.addresses.battle_cam_addr.as_mut());

        let (pitch, yaw) = calculate_length_pitch_yaw(camera_pos, target_pos);

        self.custom_camera.x = camera_pos.x_coord;
        self.custom_camera.y = camera_pos.y_coord;
        self.custom_camera.z = camera_pos.z_coord;
        self.z_diff = 0.;
        self.minimal_z = 0.;
        self.remote_data
            .remote_z
            .store(self.custom_camera.z.to_bits(), Ordering::SeqCst);
        self.custom_camera.pitch = pitch;
        self.custom_camera.yaw = yaw;
    }
}

pub struct BattlePatcher {
    patcher: LocalPatcher,
    special_patcher: LocalPatcher,
    dynamic_patches: Vec<DynamicPatch>,
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
    pub fn new(conf: &mut FreecamConfig, remote_data: &RemoteData) -> Self {
        let mut general_patcher = LocalPatcher::new();
        let mut special_patcher = LocalPatcher::new();

        // Always initialise our patcher with all the requisite patches.
        for patch in conf.patch_locations.iter_mut() {
            unsafe {
                patch_locations::patch_logic(patch, &mut general_patcher);
            }
        }

        patches::apply_general_z_remote_patch(&mut general_patcher, remote_data);
        let teleport_patch = unsafe {
            let teleport_patch = patches::create_unit_card_teleport_patch(remote_data.teleport_location.get() as usize)
                .expect("Failed to create teleport patch");
            teleport_patch.apply_to_patcher(&mut special_patcher);
            teleport_patch
        };

        Self {
            patcher: general_patcher,
            special_patcher,
            dynamic_patches: vec![teleport_patch],
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

fn calculate_length_pitch_yaw(camera_pos: &BattleCameraView, target_pos: &BattleCameraTargetView) -> (f32, f32) {
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
