use std::f32::consts::PI;
use std::time::{Duration, Instant};

use rust_hooking_utils::patching::LocalPatcher;
use rust_hooking_utils::raw_input::key_manager::{KeyState, KeyboardManager};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, VIRTUAL_KEY, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::config::FreecamConfig;
use crate::data::{BattleCameraTargetView, BattleCameraType, BattleCameraView};
use crate::mouse::ScrollTracker;
use crate::patch_locations;

pub struct BattleCamState {
    paused: bool,
    old_cursor_pos: POINT,
    velocity: Velocity,
    last_left_click: Instant,
    /// Used for the custom camera to ensure smooth motion
    custom_camera: CustomCameraState,
}

#[derive(Default)]
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

impl BattleCamState {
    pub fn new(conf: &mut FreecamConfig, patcher: &mut LocalPatcher) -> Self {
        let mut point = POINT::default();
        let _ = unsafe { GetCursorPos(&mut point) };

        // Always initialise our patcher with all the requisite patches.
        for patch in conf.patch_locations.iter_mut() {
            unsafe {
                patch_locations::patch_logic(patch, patcher);
            }
        }

        Self {
            paused: true,
            old_cursor_pos: point,
            velocity: Default::default(),
            last_left_click: Instant::now(),
            custom_camera: Default::default(),
        }
    }

    pub unsafe fn run(
        &mut self,
        patcher: &mut LocalPatcher,
        scroll: &mut ScrollTracker,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        let in_battle = *patcher.read(conf.addresses.battle_pointer.as_ref()) != 0;

        if in_battle {
            if conf.force_ttw_camera {
                // Always ensure we're on the TotalWar cam
                patcher.write(conf.addresses.battle_cam_conf_type.as_mut(), BattleCameraType::TotalWar);
            }

            if conf.camera.custom_camera_enabled {
                return self.run_battle_custom_camera(patcher, scroll, key_man, t_delta, conf);
            } else {
                return self.run_battle_no_custom(patcher, key_man, t_delta, conf);
            }
        } else {
            // If we're not in battle, obviously do nothing
            self.pause(true, patcher);
            self.sync_custom_camera(patcher, conf);
        }

        Ok(())
    }

    pub unsafe fn run_battle_no_custom(
        &mut self,
        patcher: &mut LocalPatcher,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        let target_pos = patcher.mut_read(conf.addresses.battle_cam_target_addr.as_mut());
        let camera_pos = patcher.mut_read(conf.addresses.battle_cam_addr.as_mut());
        let mut acceleration = Velocity::default();

        let (_, mut pitch, mut yaw) = calculate_length_pitch_yaw(camera_pos, target_pos);

        let mut point = POINT::default();
        GetCursorPos(&mut point)?;

        // Adjust based on free-cam movement
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.freecam_key)) {
            let invert = if conf.camera.inverted { -1.0 } else { 1.0 };
            let adjusted_sens = conf.camera.sensitivity * (1. - conf.camera.pan_smoothing);
            acceleration.pitch -= ((invert * (point.y - self.old_cursor_pos.y) as f32) / 500.) * adjusted_sens;
            acceleration.yaw -= ((invert * (point.x - self.old_cursor_pos.x) as f32) / 500.) * adjusted_sens;
        }

        println!(
            "In battle! {:#?} - {:#?}",
            patcher.read(conf.addresses.battle_cam_addr.as_ref()),
            patcher.read(conf.addresses.battle_cam_target_addr.as_ref())
        );

        // Adjust pitch and yaw
        self.velocity.pitch += acceleration.pitch;
        self.velocity.yaw += acceleration.yaw;
        pitch += self.velocity.pitch;
        yaw += self.velocity.yaw;
        // println!("Pitch: {} - Yaw: {}", pitch, yaw);

        self.velocity.pitch *= conf.camera.pan_smoothing;
        self.velocity.yaw *= conf.camera.pan_smoothing;

        // Write to the addresses
        if !self.paused {
            write_pitch_yaw(camera_pos, target_pos, pitch, yaw);
        } else {
            // Update
            self.sync_custom_camera(patcher, conf);
        }

        // Persist info for next loop
        self.old_cursor_pos = point;
        Ok(())
    }

    unsafe fn run_battle_custom_camera(
        &mut self,
        patcher: &mut LocalPatcher,
        scroll: &mut ScrollTracker,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        let target_pos = patcher.mut_read(conf.addresses.battle_cam_target_addr.as_mut());
        let camera_pos = patcher.mut_read(conf.addresses.battle_cam_addr.as_mut());
        let mut acceleration = Velocity::default();
        let (horizontal_speed, vertical_speed) = calculate_speed_multipliers(conf, key_man);

        let (pitch, yaw) = (self.custom_camera.pitch, self.custom_camera.yaw);

        let mut point = POINT::default();
        GetCursorPos(&mut point)?;

        // Detect double click (vanilla functionality retention)
        if key_man.get_key_state(VK_LBUTTON) == KeyState::Pressed {
            let now = Instant::now();
            let time_since_last = now.duration_since(self.last_left_click);
            self.last_left_click = now;

            println!(
                "Time since last left: {:#?} and double: {:#?}",
                time_since_last,
                GetDoubleClickTime()
            );
            println!("Old Cursor: {:#?} - New: {:#?}", self.old_cursor_pos, point);

            if (time_since_last.as_millis() as u32) < GetDoubleClickTime()
                && (self.old_cursor_pos.x - point.x).abs() < 10
                && (self.old_cursor_pos.y - point.y).abs() < 10
            {
                println!("Pausing!");
                self.pause(true, patcher);
            }
        }

        // Adjust based on free-cam movement
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.freecam_key)) {
            let invert = if conf.camera.inverted { -1.0 } else { 1.0 };
            let adjusted_sens = conf.camera.sensitivity * (1. - conf.camera.pan_smoothing);
            acceleration.pitch -= ((invert * (point.y - self.old_cursor_pos.y) as f32) / 500.) * adjusted_sens;
            acceleration.yaw -= ((invert * (point.x - self.old_cursor_pos.x) as f32) / 500.) * adjusted_sens;
            // We should have control again.
            self.pause(false, patcher);
        }

        // Camera movement
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.forward_key)) {
            acceleration.y += yaw.sin();
            acceleration.x += yaw.cos();
            self.pause(false, patcher);
        }
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.backwards_key)) {
            acceleration.y += (PI + yaw).sin();
            acceleration.x += (PI + yaw).cos();
            self.pause(false, patcher);
        }
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.left_key)) {
            acceleration.y += ((PI / 2.) + yaw).sin();
            acceleration.x += ((PI / 2.) + yaw).cos();
            self.pause(false, patcher);
        }
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.right_key)) {
            acceleration.y += ((3. * PI / 2.) + yaw).sin();
            acceleration.x += ((3. * PI / 2.) + yaw).cos();
            self.pause(false, patcher);
        }

        // Rotation controls
        let pan_speed = 1. - conf.camera.pan_smoothing;
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.rotate_left)) {
            acceleration.yaw += 0.03 * pan_speed;
            self.pause(false, patcher);
        }
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.rotate_right)) {
            acceleration.yaw -= 0.03 * pan_speed;
            self.pause(false, patcher);
        }

        // Handle scroll TODO: Figure out how this works.
        let scroll_delta = scroll.get_scroll_delta() * if conf.camera.inverted_scroll { -1 } else { 1 };
        let is_negative = if scroll_delta != 0 { scroll_delta.abs() / scroll_delta } else { 1 };
        self.velocity.z += (scroll_delta.pow(2) * is_negative) as f32 * vertical_speed / 10.;

        // Write the new camera values
        let mut length = (acceleration.x.powi(2) + acceleration.y.powi(2) + acceleration.z.powi(2)).sqrt();
        if length == 0. {
            length = 1.;
        }

        self.velocity.x +=
            ((acceleration.x / length) * (horizontal_speed * (1. - conf.camera.horizontal_smoothing))) / 2.;
        self.velocity.y +=
            ((acceleration.y / length) * (horizontal_speed * (1. - conf.camera.horizontal_smoothing))) / 2.;
        self.velocity.z += ((acceleration.z / length) * (vertical_speed * (1. - conf.camera.vertical_smoothing))) / 2.;
        // Freecam
        self.velocity.pitch += acceleration.pitch;
        self.velocity.yaw += acceleration.yaw;

        self.custom_camera.x += self.velocity.x;
        self.custom_camera.y += self.velocity.y;
        self.custom_camera.z += self.velocity.z;
        self.custom_camera.pitch += self.velocity.pitch;
        self.custom_camera.yaw += self.velocity.yaw;

        self.velocity.x *= conf.camera.horizontal_smoothing;
        self.velocity.y *= conf.camera.horizontal_smoothing;
        self.velocity.z *= conf.camera.vertical_smoothing;
        self.velocity.pitch *= conf.camera.pan_smoothing;
        self.velocity.yaw *= conf.camera.pan_smoothing;

        // println!(
        //     "In battle! {:#?} - {:#?}",
        //     patcher.read(conf.addresses.battle_cam_addr.as_ref()),
        //     patcher.read(conf.addresses.battle_cam_target_addr.as_ref())
        // );
        //
        // println!("Pitch: {} - Yaw: {}", pitch, yaw);

        self.custom_camera.x = 900.0f32.min((-900.0f32).max(self.custom_camera.x));
        self.custom_camera.y = 900.0f32.min((-900.0f32).max(self.custom_camera.y));

        // Write to the addresses
        if !self.paused {
            // Important that this runs _before_ pitch/yaw adjustment as they're dependent.
            write_custom_camera(&self.custom_camera, camera_pos);
            write_pitch_yaw(camera_pos, target_pos, pitch, yaw);
        } else {
            // Update
            self.sync_custom_camera(patcher, conf);
        }

        // Persist info for next loop
        self.old_cursor_pos = point;
        Ok(())
    }

    unsafe fn pause(&mut self, paused: bool, patcher: &mut LocalPatcher) {
        if self.paused != paused {
            self.paused = paused;
            if paused {
                patcher.disable_all_patches();
            } else {
                patcher.enable_all_patches();
            }
        }
    }

    unsafe fn sync_custom_camera(&mut self, patcher: &LocalPatcher, conf: &mut FreecamConfig) {
        let target_pos = patcher.mut_read(conf.addresses.battle_cam_target_addr.as_mut());
        let camera_pos = patcher.mut_read(conf.addresses.battle_cam_addr.as_mut());

        let (_, pitch, yaw) = calculate_length_pitch_yaw(camera_pos, target_pos);

        self.custom_camera.x = camera_pos.x_coord;
        self.custom_camera.y = camera_pos.y_coord;
        self.custom_camera.z = camera_pos.z_coord;
        self.custom_camera.pitch = pitch;
        self.custom_camera.yaw = yaw;
    }
}

fn write_pitch_yaw(
    camera_pos: &mut BattleCameraView,
    target_pos: &mut BattleCameraTargetView,
    mut pitch: f32,
    yaw: f32,
) {
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

fn calculate_length_pitch_yaw(camera_pos: &BattleCameraView, target_pos: &BattleCameraTargetView) -> (f32, f32, f32) {
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

    (length, pitch, yaw)
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
