use std::f32::consts::PI;
use std::time::Duration;

use rust_hooking_utils::patching::LocalPatcher;
use rust_hooking_utils::raw_input::key_manager::KeyboardManager;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::config::FreecamConfig;
use crate::data::{BattleCameraTargetView, BattleCameraType, BattleCameraView};
use crate::mouse::ScrollTracker;

pub struct BattleCamState {
    active: bool,
    old_cursor_pos: POINT,
    velocity: Velocity,
}

#[derive(Default)]
pub struct Velocity {
    x: f32,
    y: f32,
    z: f32,
    pitch: f32,
    yaw: f32,
}

impl BattleCamState {
    pub fn new() -> Self {
        let mut point = POINT::default();
        let _ = unsafe { GetCursorPos(&mut point) };

        Self {
            active: true,
            old_cursor_pos: point,
            velocity: Default::default(),
        }
    }

    pub unsafe fn run(
        &mut self,
        patcher: &LocalPatcher,
        scroll: &mut ScrollTracker,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        let in_battle = *patcher.read(conf.addresses.battle_pointer.as_ref()) != 0;

        if in_battle {
            self.run_battle(patcher, scroll, key_man, t_delta, conf)
        } else {
            // If we're not in battle, obviously do nothing
            Ok(())
        }
    }

    unsafe fn run_battle(
        &mut self,
        patcher: &LocalPatcher,
        scroll: &mut ScrollTracker,
        key_man: &mut KeyboardManager,
        t_delta: Duration,
        conf: &mut FreecamConfig,
    ) -> anyhow::Result<()> {
        if conf.override_default_camera {
            // Always ensure we're on the TotalWar cam
            patcher.write(conf.addresses.battle_cam_conf_type.as_mut(), BattleCameraType::TotalWar);
        }

        let target_pos = patcher.mut_read(conf.addresses.battle_cam_target_addr.as_mut());
        let camera_pos = patcher.mut_read(conf.addresses.battle_cam_addr.as_mut());
        let mut acceleration = Velocity::default();

        let (length, mut pitch, mut yaw) = calculate_length_pitch_yaw(&camera_pos, &target_pos);

        let mut point = POINT::default();
        GetCursorPos(&mut point)?;

        // Adjust based on free-cam movement
        if key_man.has_pressed(VIRTUAL_KEY(conf.keybinds.freecam_key)) {
            let invert = if conf.camera.inverted { -1.0 } else { 1.0 };
            let adjusted_sens = conf.camera.sensitivity * (1. - conf.camera.smoothing);
            acceleration.pitch -= (invert * (point.y - self.old_cursor_pos.y) as f32) / ((500.) * adjusted_sens);
            acceleration.yaw -= (invert * (point.x - self.old_cursor_pos.x) as f32) / ((500.) * adjusted_sens);
            // We should have control again.
            self.active = true;
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
        println!("Pitch: {} - Yaw: {}", pitch, yaw);

        self.velocity.pitch *= conf.camera.smoothing;
        self.velocity.yaw *= conf.camera.smoothing;

        // Write to the addresses
        write_pitch_yaw(camera_pos, target_pos, pitch, yaw);

        // Persist info for next loop
        self.old_cursor_pos = point;
        Ok(())
    }
}

fn write_pitch_yaw(
    camera_pos: &mut BattleCameraView,
    target_pos: &mut BattleCameraTargetView,
    mut pitch: f32,
    mut yaw: f32,
) {
    pitch = pitch.max(-(PI / 2.) * 0.9);
    pitch = pitch.min((PI / 2.) * 0.9);

    target_pos.x_coord = (yaw.cos() * pitch.cos() * 1000.) + camera_pos.x_coord;
    target_pos.y_coord = (yaw.sin() * pitch.cos() * 1000.) + camera_pos.y_coord;
    target_pos.z_coord = (pitch.sin() * 1000.) + camera_pos.z_coord;
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
