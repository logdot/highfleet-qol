# Highfleet QOL
Simple mod with various QOL features.
It is intended to be used with the [Highfleet Modloader](https://github.com/logdot/Highfleet-Modloader).

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
  "enable_anti_wobble": false,
  "enable_unblocked_guns": false,
  "enable_reduced_shake": false,
  "enable_flare_crash_fix": true,
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
1. Anti Wobble: Custom GUI elements in the battle screen will no longer shake.
2. Unblocked Guns: Hull and other components no longer block weapons. Only does anything in 1.151 since this is the default behaviour in 1.163.
3. Reduced Shake: Greatly reduces the amount of screen shake in the battle screen, e.g. when firing weapons or using thrusters.
4. Flare Crash Fix: Prevents the 1.163 missile-fuze crash when a flare's linked object is no longer available. Enabled by default and not required on 1.151.
5. Arcade Zoom: Unlocks zooming in and out in the battle screen.
     * Max Zoom: The maximum zoom level.
     * Min Zoom: The minimum zoom level (must be at least 0). This will be the default zoom when entering a battle.
     * Zoom levels: List of each zoom value. You can define as many zoom levels as you want. The first value is zoom level 0.

## Refuel rate and speed curve (1.163 only)

Two optional changes for the strategic map. Both are **off by default**, each has its own section in `qol.json`, and they only do anything on Steam 1.163 (on 1.151 they are ignored and a warning is logged).

* **Refuel rate**: fleets take fuel from a station faster (or slower).
* **Speed curve**: changes how fast a fleet cruises on the map for a given thrust-to-weight ratio (TWR) of its ships.

### Turning them on, step by step

1. Start the game once with the mod installed. This creates `Modloader/config/qol.json` with a `refuel` and a `speed` section (both `"enabled": false`). If your `qol.json` already existed from an older version, it is not rewritten: add the sections below by hand. A missing section simply means the feature stays off.
2. Close the game, open `Modloader/config/qol.json` and set `"enabled": true` in the section you want, plus the numbers (example below).
3. Start the game. Settings are read once at start-up, so restart after every change.
4. Check `Modloader/logs/Highfleet_<date>.log`. A working setup logs lines like `Refuel rate patch prepared at 0x14011521f (200 -> 2000)` and `Speed curve patch prepared at 0x140080699 (50 * max(TWR - 0, 0) ^ 1)`. If a value is out of range the log says so and the feature stays off.
5. Refuel: dock at a station with a part-empty tank and watch the fuel gauge. Speed: open a ship in the editor (or launch a fleet). The game caches each ship's speed, so reopen or edit the ship to see the new number.

If you used the standalone `refuel.dll` or `speed.dll` mods, remove them: both patch the same game code, and this section replaces them (`refuel.ini` and the old speed settings are no longer read).

```json
{
  "refuel": {
    "enabled": true,
    "player_rate_multiplier": 4.0,
    "ai_rate_multiplier": 1.0
  },
  "speed": {
    "enabled": true,
    "preset": "classic",
    "coefficient": 25.0,
    "subtract": 0.0,
    "exponent": 1.0,
    "max_kmh": 0
  }
}
```

* **Refuel**: multiplies how fast fleets take fuel at stations (`1.0` is normal, limited to `0.01` through `10000`). A huge value just means "fill up at once"; the amount is still capped by free tank space and station stock. The AI fleets use the same code, so `ai_rate_multiplier` changes how fast they refuel.
* **Speed**: replaces the map cruise speed formula `speed (m/s) = coefficient * max(TWR - subtract, 0) ^ exponent` (the editor shows km/h, which is m/s times 3.6). The 1.163 game uses `50 * sqrt(TWR - 1)`, which is `coefficient 50, subtract 1, exponent 0.5`. `"preset": "classic"` is the linear 1.151 curve, `25 * TWR`. `"preset": "custom"` uses the three numbers. `max_kmh` caps the speed (`0` is no cap). The game caches each ship's speed, so reopen or edit a ship to see a change, and restart the game after editing the config.

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

### Aircraft gun ammo

A loadout can use `gun_ammo` to select any aircraft-gun ammo definition available when the plane launches, including definitions injected by another mod.

Omit `gun_ammo` (or set it to `null`) for a loadout without a gun. Existing configurations using `"has_gun37mm": true` remain supported and are interpreted as `"gun_ammo": "ITEM_GUN37"`.

The selected definition must be an aircraft-gun ammo (`reticle` 4) with a positive gun capacity. If the definition is missing or invalid, the game falls back to `ITEM_GUN37` for that launch.
