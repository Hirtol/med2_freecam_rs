# Medieval 2 Free Camera RS
A Rust implementation of the [Medieval 2 Freecam](https://www.moddb.com/mods/freecam-medieval-2) mod with a few enhancements.

## Enhancements
* With the option `prevent_ground_clipping` set to `true` the camera will no longer go below the ground. The exact 'ground margin' can also be changed.
* With the option `maintain_relative_height` set to `true` the camera will (similar to base-game and the Warhammer TTW titles) stay at a consistent relative elevation above the ground, even while you move over mountains/valleys.
* Hide the mouse cursor while rotating the camera using the `freecam` button. 
* Allow the middle mouse button to be used for `freecam` movement.
* Double clicking unit cards is no longer subject to a race condition which occasionally caused the camera to go wild.
* Shipped as a proxied DLL, only requiring the DLL to be inserted into the game's folder and any mod will automatically have the code injected. No need to launch a separate program.
* Force the user's camera to the `TotalWar Camera` to prevent issues when the user forgets to switch off `RTS Camera`.

## How to use
* First, download the latest release [here](https://github.com/Hirtol/med2_freecam_rs/releases).
* Navigate to your Medieval 2 Total War folder, the same place where the `medieval2.exe` is located
* Unzip the contents of `freecam-rs-i686-pc-windows-msvc.zip` downloaded prior in the Medieval 2 folder.
* Run the game once, the `freecam_config.json` will now have been generated, you can tweak it to your liking.

### Config Description

```json5
{
  // Debug console, if you don't know what it is, just leave it as `false`    
  "console": false,
  // How frequently to run the camera movement code. Keep this > 60  
  "update_rate": 144,
  // All keys to press to reload the config while the game is running  
  "reload_config_keys": [
    "VK_CONTROL",
    "VK_SHIFT",
    "VK_R"
  ],
  // The panning/custom camera only work if the game has been set to the TotalWar Camera
  // Leave this on `true`
  "force_ttw_camera": true,
  // This blocks the base game's middle mouse click during battles 
  // to allow it to be used for Freecam instead.
  "block_game_middle_mouse_functionality": true,
  // All relevant keys, to see available key names refer to: 
  // https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes
  "keybinds": {
    "fast_key": "VK_SHIFT",
    "slow_key": "VK_MENU",
    "freecam_key": "VK_MBUTTON",
    "forward_key": "VK_W",
    "backwards_key": "VK_S",
    "left_key": "VK_A",
    "right_key": "VK_D",
    "rotate_left": "VK_Q",
    "rotate_right": "VK_E"
  },
  "camera": {
    // Whether to use the custom camera (Warhammer like) movement or not.
    "custom_camera_enabled": true,
    "inverted": false,
    "inverted_scroll": true,
    /// Whether to emulate Warhammers movement, where the camera moves slower when you're closer to the ground.
    "ground_distance_speed": true,
    "sensitivity": 1.0,
    // `Cinematic Smoothing` is what these values are called in Warhammer, higher values
    // mean slower movement decay. Should always be less than `1.0`.
    "rotate_smoothing": 0.75,
    "vertical_smoothing": 0.92,
    "horizontal_smoothing": 0.92,
    // Base movement speed, if it's too slow/fast for your liking tweak these up/down
    "horizontal_base_speed": 1.0,
    "vertical_base_speed": 1.0,
    // How much slower to move while the `slow_key` is pressed   
    "slow_multiplier": 0.2,
    // How much faster to move while the `fast_key` is pressed
    "fast_multiplier": 3.5,
    // When moving across uneven terrain this will force your camera to move down/up (relatively)
    // with the terrain like in Warhammer/base game Medieval 2
    "maintain_relative_height": true,
    // Used to ensure camera stability during unit/map panning. Leave this as is
    "relative_height_panning_delay": {
      "secs": 0,
      "nanos": 25000000
    },
    // Whether to prevent camera ground clipping. Setting this to `false` will allow you to
    // go under the map
    "prevent_ground_clipping": true,
    // How much margin to leave above the ground if `prevent_ground_clipping` is on.
    // If this is set too low you will partially clip into mountains/uneven terrain while moving close to the ground.
    "ground_clip_margin": 1.3
  }
}
```

## How to remove
Simply delete the `version.dll` file which you inserted into the Medieval 2 folder.

## Requirements

* Steam version of Medieval 2

## Developing

* Nightly Rust toolchain required
* Run `cargo build --target i686-pc-windows-msvc --release` to build it yourself.

## Credits
* Bugis_Duckis - Significant parts of the custom movement code and most of the game's camera addresses were provided in the original CPP source.