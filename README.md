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
4. Acade Zoom: Unlocks zooming in and out in the battle screen.
     * Max Zoom: The maximum zoom level.
     * Min Zoom: The minimum zoom level (must be at least 0). This will be the default zoom when entering a battle.
     * Zoom levels: List of each zoom value. You can define as many zoom levels as you want. The first value is zoom level 0.


