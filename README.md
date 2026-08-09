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

## Aircraft gun ammo

A plane loadout can use `gun_ammo` to select any aircraft-gun ammo definition available when the plane launches, including definitions injected by another mod:

```json
{
  "oid": "LOADOUT_T7_GUN57_K13",
  "icon": "LOADOUT_K13",
  "vec_parts": [
    { "name": "ITEM_K13", "count": 2 }
  ],
  "launch_loadout_weight": 10,
  "gun_ammo": "ITEM_GUN57"
}
```

Omit `gun_ammo` (or set it to `null`) for a loadout without a gun. Existing configurations using `"has_gun37mm": true` remain supported and are interpreted as `"gun_ammo": "ITEM_GUN37"`.

The selected definition must be an aircraft-gun ammo (`reticle` 4) with a positive gun capacity. If the definition is missing or invalid, the game falls back to `ITEM_GUN37` for that launch.
