use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use log::LevelFilter;
use rust_hooking_utils::raw_input::key_manager::KeyboardManager;
use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;

use crate::battle_cam::BattleCamera;
use crate::config::FreecamConfig;
use crate::mouse::ScrollTracker;
use crate::patcher::LocalPatcher;

mod config;
mod data;
mod mouse;
mod ptr;

mod battle_cam;
mod patcher;

static SHUTDOWN_FLAG: AtomicBool = AtomicBool::new(false);

pub fn dll_attach(hinst_dll: windows::Win32::Foundation::HMODULE) -> Result<()> {
    let dll_path = rust_hooking_utils::get_current_dll_path(hinst_dll)?;
    let config_directory = dll_path.parent().context("DLL is in root")?;
    let cfg = simplelog::ConfigBuilder::new().build();

    // Ignore result in case we have double initialisation of the DLL.
    simplelog::SimpleLogger::init(LevelFilter::Trace, cfg)?;

    config::create_initial_config(config_directory)?;

    let mut conf = config::load_config(config_directory)?;

    if conf.console {
        unsafe {
            windows::Win32::System::Console::AllocConsole()?;
        }
    }

    log::info!("Loaded config: {:#?}", conf);

    let mut key_manager = KeyboardManager::new();
    let mut update_duration = Duration::from_secs_f64(1.0 / conf.update_rate as f64);
    let mut scroll_tracker = ScrollTracker::new()?;
    let mut patcher = rust_hooking_utils::patching::LocalPatcher::new();
    let mut battle_cam = BattleCamera::new(LocalPatcher::new());

    let mut last_update = Instant::now();

    while !SHUTDOWN_FLAG.load(Ordering::Acquire) {
        if let Some(reload) = &conf.reload_config_keys {
            if key_manager.all_pressed(reload.iter().copied().map(VIRTUAL_KEY)) {
                conf = reload_config(config_directory, &mut conf, &mut battle_cam)?;
                update_duration = Duration::from_secs_f64(1.0 / conf.update_rate as f64);
            }

            unsafe {
                battle_cam.run(&mut conf, &mut scroll_tracker, &mut key_manager, last_update.elapsed())?;
            }

            last_update = Instant::now();
        }

        std::thread::sleep(update_duration);
        key_manager.end_frame();
    }

    Ok(())
}

pub fn dll_detach(_hinst_dll: windows::Win32::Foundation::HMODULE) -> Result<()> {
    SHUTDOWN_FLAG.store(true, Ordering::SeqCst);
    log::info!("Detached! {:?}", std::thread::current().id());

    Ok(())
}

fn reload_config(
    config_dir: impl AsRef<Path>,
    old: &mut FreecamConfig,
    battle_cam: &mut BattleCamera,
) -> anyhow::Result<FreecamConfig> {
    log::debug!("Reloading config");
    let mut conf = config::load_config(config_dir)?;

    // Open/close console
    if old.console && !conf.console {
        unsafe {
            windows::Win32::System::Console::FreeConsole()?;
        }
    } else if !old.console && conf.console {
        unsafe {
            windows::Win32::System::Console::AllocConsole()?;
        }
    }

    if old.camera.custom_camera_enabled && !conf.camera.custom_camera_enabled {
        battle_cam.set_custom_camera(false);
    } else if !old.camera.custom_camera_enabled && conf.camera.custom_camera_enabled {
        battle_cam.set_custom_camera(true);
    }

    log::debug!("New config loaded: {:#?}", conf);

    Ok(conf)
}
