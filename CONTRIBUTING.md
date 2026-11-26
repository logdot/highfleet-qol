# Contributing

Thank you for contributing to `highfleet-qol`.
These guidelines reflect the conventions and review expectations for this repository.

## PR Scope and Hygiene

- Do not add a `CHANGELOG.md` file.
- Do not bump the crate version in `Cargo.toml` in feature or bugfix PRs.
- Keep PRs focused and reasonably sized so they are easy to review.
- Test your changes in-game before requesting review.
- When a change targets both Steam 1.163 and 1.151, verify addresses, bytes, and struct differences on both versions.

## Architecture and Dependencies

- Minimize low-level OS dependencies (such as `windows-sys`) inside the mod.
- Prefer `patchy-rs` helpers (such as `patchy::ProcessModule` and `session.overwrite(...)`) over manual `GetModuleHandleA` or `VirtualQuery` checks, and upstream reusable low-level utilities to `patchy-rs` when appropriate.
- Use `highfleet-rs` types instead of raw `*mut u8` / `*const u8` pointers and hardcoded field offsets.
- Prefer reading game globals directly in Rust over scraping values from registers at a jump site.
- Keep hooks minimal and localized to the exact site needed rather than hooking broad dispatchers or maintaining complex state-tracking hooks.

## Version Detection and Patch Safety

- Rely on Modloader's executable hash and reported game version rather than adding redundant per-site vanilla-byte signatures or checks.
- Do not attempt to detect external patchers or modified executables; external patchers and unsupported builds are unsupported.
- Treat Modloader's version result as advisory, not an authorization or safety guarantee: on an unknown, mismatched, or unsupported build, the Modloader warns and still attempt the normal initialization path using the compiled patch addresses and layouts.
- In short, don't be overly defensive checking bytes or repeating other checks that the modloader already gives you.
  You can assume the file you are patching is exactly as you expect it to be.
  If it is not, we consider that a sort of UB and we carry no guarantees.

## Configuration Design

- Avoid adding an `enabled` boolean when a config section can default to vanilla behavior through its values (such as `1.0` multipliers or a `Vanilla` preset).
- Reserve `enabled` toggles for features that have no vanilla-equivalent default value.
- When behavior differs across game versions, use a `Vanilla` default variant that resolves to the running game version, alongside explicit presets such as `Classic` (1.151) and `Modern` (1.163).
- Validate or normalize config values during deserialization (for example with `serde`'s `deserialize_with`) so callers can read fields directly without remembering to call helper functions.
- Do not add arbitrary min/max clamps or silent fallbacks to vanilla values unless an out-of-range or empty value would actually crash the game.

## Code Style

- Break up functions with blank lines between logical steps instead of writing dense walls of code.
- Name functions after what they actually do, and split distinct responsibilities into separate functions.
- Add explanatory comments for magic numbers (such as bitmasks like `0x8000`) and non-obvious behavior.
- Run `cargo fmt` before pushing.

## Documentation and Comments

- Write one sentence per line in `README.md` and in Rust doc comments (`///`).
- Add new options to the appropriate user-facing section of `README.md` (such as the list of toggles).
- Keep internal implementation rationale and notes about omitted or unimplemented proposals out of `README.md`.
- Make sure doc comments on config fields and enum variants clearly explain what every option does.
