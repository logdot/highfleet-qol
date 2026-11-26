# Highfleet QOL

Simple mod with various QOL features.
It is intended to be used with the [Highfleet Modloader](https://github.com/logdot/Highfleet-Modloader).

QOL requires a Highfleet Modloader build with the shared Patchy service.
Use the QOL build that matches your Steam executable (1.151 or 1.163).
On unsupported versions or externally patched executables, QOL warns and still attempts to initialize.

## Building from source

Initialize the `highfleet-rs` submodule before building with Cargo.
For a fresh clone, use `git clone --recurse-submodules https://github.com/logdot/highfleet-qol.git`.
For an existing checkout, use `git submodule update --init --recursive`.
Cargo uses the local submodule through a path dependency, and the submodule pins the shared library revision.

## Installation

To install, download the mod from [releases](https://github.com/logdot/highfleet-qol/releases/latest).
The zip has two folders, one each for the 1.151 and 1.163 versions of the game.
Open the respective folder for your version.
There should be a `Modloader` folder inside.
Move the `Modloader` folder (not it's contents) into the root of your highfleet installation.

Your Highfleet folder should look like this:

<img width="609" height="457" alt="Screenshot 2025-10-14 at 7 10 37 PM" src="https://github.com/user-attachments/assets/7dea0627-7021-4f3b-b369-ada27aea8c98" />

When the mod first runs, it generates a default config file in `Modloader/config/qol.json`.
This is what the file looks like by default:

```json
{
  "enable_plane_icons_rework": true,
  "enable_anti_wobble": false,
  "enable_unblocked_guns": false,
  "enable_reduced_shake": false,
  "enable_flare_crash_fix": true,
  "enable_antitilt_fps_fix": true,
  "enable_intro": false,
  "enemy_laser_warning": true,
  "enemy_sound_warning": true,
  "enable_arcade_zoom": true,
  "max_zoom_level": 5,

  "min_zoom_level": 3,
  "zoom_levels": [
    14.0,
    7.0,
    1.0,
    0.7,
    0.5,
    0.3
  ]
}
```

The list of toggles is:
1. Plane Icons Rework: Shows every aircraft type listed in `planes` in the battle aircraft panel, not just the T-7 and LA-29. Each type needs an animation named `sign_<aircraft name>`; a type without one still shows its count, with an empty slot for the icon. See [Aircraft icons](#aircraft-icons). Enabled by default, Steam 1.151 and 1.163.
2. Anti Wobble: Custom GUI elements in the battle screen will no longer shake.
3. Unblocked Guns: Hull and other components no longer block weapons. Only does anything in 1.151 since this is the default behaviour in 1.163.
4. Reduced Shake: Greatly reduces the amount of screen shake in the battle screen, e.g. when firing weapons or using thrusters.
5. Flare Crash Fix: Prevents the 1.163 missile-fuze crash when a flare's linked object is no longer available. Enabled by default and not required on 1.151.
6. Arcade Zoom: Unlocks zooming in and out in the battle screen.
     * Max Zoom: The maximum zoom level.
     * Min Zoom: The minimum zoom level (must be at least 0). This will be the default zoom when entering a battle.
     * Zoom levels: List of each zoom value. You can define as many zoom levels as you want. The first value is zoom level 0.
7. Elevation Rules (Steam 1.163 only): Set `"elevation_rules": "classic"` to restore the 1.151 rules.
   These cover free-standing parts, engines and the wider KAZ intercept cone.
   For the full 1.151 KAZ, also edit `MDL_KAZ` in `OL.seria`.
   Set `m_mdl_sectors_type=3`, `m_mdl_sectors_threshold=0.5` and `m_mdl_sectors_shift=1`.
   The default `"vanilla"` and `"modern"` leave the 1.163 rules unchanged.
8. Intro: On Steam 1.163, set `"enable_intro": true` in `qol.json` to restore the native startup logos.
   After the first second, a fresh key press or mouse click skips them.
   Remove the standalone hf_intro DLL before enabling this option.
8. Pre-Fire Warnings (Steam 1.163 only): Set `"enemy_laser_warning": false` to hide gun pre-fire warning lines for both sides, including friendly and allied ships.
   Set `"enemy_sound_warning": false` to mute the enemy pre-fire sound warning.
   Both default to `true` and work independently.
9. Anti-Tilt FPS Fix: Raising `CORE_FRAMERATE` in `Config.ini` makes the engines level a ship faster than the game intends.
   QOL keeps the levelling the same at any frame rate, and nothing changes at 50 fps.
   Compatible with `hf_wish_tilt`; set `"enable_antitilt_fps_fix": false` if another mod should apply the scaling instead.

## Refuel rate and speed curve (1.163 only)

Two optional changes for the strategic map. Both **keep the game's own behaviour by default**, each has its own section in `qol.json`, and they only do anything on Steam 1.163 (on 1.151 they are ignored and a warning is logged).

* **Refuel rate**: fleets take fuel from a station faster (or slower).
* **Speed curve**: changes how fast a fleet cruises on the map for a given thrust-to-weight ratio (TWR) of its ships.

### Turning them on, step by step

1. Start the game once with the mod installed. This creates `Modloader/config/qol.json` with a `refuel` and a `speed` section holding the game's own values. If your `qol.json` already existed from an older version, it is not rewritten: add the sections below by hand. A missing section simply means the game's own behaviour.
2. Close the game, open `Modloader/config/qol.json` and change the numbers in the section you want (example below).
3. Start the game. Settings are read once at start-up, so restart after every change.
4. Check `Modloader/logs/Highfleet_<date>.log`. A working setup logs lines like `Refuel rate patch prepared at 0x14011521f (200 -> 2000)` and `Speed curve patch prepared at 0x140080699 (50 * max(TWR - 0, 0) ^ 1)`. If a value is out of range the log says so and the game's own behaviour stays.
5. Refuel: dock at a station with a part-empty tank and watch the fuel gauge. Speed: open a ship in the editor (or launch a fleet). The game caches each ship's speed, so reopen or edit the ship to see the new number.

If you used the standalone `refuel.dll` or `speed.dll` mods, remove them: both patch the same game code, and this section replaces them (`refuel.ini` and the old speed settings are no longer read).

```json
{
  "refuel": {
    "player_rate_multiplier": 4.0,
    "ai_rate_multiplier": 1.0
  },
  "speed": {
    "preset": "custom",
    "coefficient": 25.0,
    "subtract": 0.0,
    "exponent": 1.0,
    "max_kmh": 0
  }
}
```

* **Refuel**: multiplies how fast fleets take fuel at stations (`1.0` is normal; `0` stops refuelling and a negative value drains fuel). A huge value just means "fill up at once"; the amount is still capped by free tank space and station stock. The AI fleets use the same code, so `ai_rate_multiplier` changes how fast they refuel.
* **Speed**: replaces the map cruise speed formula `speed (m/s) = coefficient * max(TWR - subtract, 0) ^ exponent` (the editor shows km/h, which is m/s times 3.6). The 1.163 game uses `50 * sqrt(TWR - 1)`, which is `coefficient 50, subtract 1, exponent 0.5`. `"preset"` picks the curve: `"vanilla"` (the default) is the game's own curve for the running version, `"classic"` is the linear 1.151 curve `25 * TWR`, `"modern"` is the 1.163 curve, and `"custom"` uses the three numbers. Only 1.163 is supported for now, so `vanilla` is the same as `modern`. `max_kmh` caps the speed (`0` is no cap). The game caches each ship's speed, so reopen or edit a ship to see a change, and restart the game after editing the config.

## Aircraft settings

Each entry in `planes` can set the aircraft's health and available loadouts. The generated configuration contains the correct internal aircraft names to edit:

```json
{
  "planes": {
    "CRAFT_T7": {
      "health": 45.0,
      "loadouts": [
        {
          "oid": "LOADOUT_T7_GUN57_K13",
          "icon": "LOADOUT_K13",
          "vec_parts": [
            { "name": "ITEM_K13", "count": 2 }
          ],
          "launch_loadout_weight": 10,
          "gun_ammo": "ITEM_GUN57"
        }
      ]
    }
  }
}
```

`health` sets both current and maximum health whenever that aircraft launches. It must be greater than zero; missing or invalid values use the game's normal `30.0` health.

Older configurations that store each plane as a loadout array are still accepted and use `30.0` health automatically.

### Aircraft icons

The battle screen's aircraft panel draws one icon per aircraft type from an animation named `sign_<aircraft name>`, the same convention the game uses for its own two types: `CRAFT_T7` uses `sign_T7`, `CRAFT_LA29` uses `sign_LA29`, so a `CRAFT_SB15` needs `sign_SB15`. Add the animation to any `.res` file included by `Media/media.res` and point it at whatever texture holds the art:

```plaintext
Animation sign_SB15
{
 texture = Static6
 rect = 3986,211,98,98
 hotspot = 49,49
 frames = 1
}
```

`sign_T7` is 58 x 73; art larger than that is shrunk to fit, so a 98 x 98 icon also works. A type whose animation is missing is still listed with its count, but its icon slot stays empty. Names are logged once when the panel first asks for them.

### Aircraft gun ammo

A loadout can use `gun_ammo` to select any aircraft-gun ammo definition available when the plane launches, including definitions injected by another mod.

Omit `gun_ammo` (or set it to `null`) for a loadout without a gun. Existing configurations using `"has_gun37mm": true` remain supported and are interpreted as `"gun_ammo": "ITEM_GUN37"`.

The selected definition must be an aircraft-gun ammo (`reticle` 4) with a positive gun capacity. If the definition is missing or invalid, the game falls back to `ITEM_GUN37` for that launch.
