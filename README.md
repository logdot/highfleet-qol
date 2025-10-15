# Highfleet QOL
Simple mod with various QOL features.
It is intended to be used with the [Highfleet Modloader](https://github.com/logdot/Highfleet-Modloader).


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


