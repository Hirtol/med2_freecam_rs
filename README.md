# Medieval 2 Free Camera RS
A Rust implementation of the [Medieval 2 Freecam](https://www.moddb.com/mods/freecam-medieval-2) mod with a few enhancements.

## Enhancements
* With the option `prevent_ground_clipping` set to `true` the camera will no longer go below the ground. The exact 'ground margin' can also be changed.
* With the option `maintain_relative_height` set to `true` the camera will (similar to base-game and the Warhammer TTW titles) stay at a consistent relative elevation above the ground, even while you move over mountains/valleys.
* Double clicking unit cards is no longer subject to a race condition which occasionally caused the camera to go wild.
* Shipped as a proxied DLL, only requiring the DLL to be inserted into the game's folder and any mod will automatically have the code injected. No need to launch a separate program.
* Force the user's camera to the `TotalWar Camera` to prevent issues when the user forgets to switch off `RTS Camera`.

## Requirements

* Steam version of Medieval 2

## Credits
* Bugis_Duckis - Significant parts of the custom movement code and most of the game's camera addresses were provided in the original CPP source.