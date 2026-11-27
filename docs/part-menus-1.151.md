# Part menus on Steam 1.151 тАФ research and implementation roadmap

Status: the feature is implemented and play-tested on **Steam 1.163 only**
(`src/part_menus.rs`). qol's `1_151` feature still compiles because the module is
gated off with `#[cfg(not(feature = "1_151"))]`. This file is the plan for a
1.151 port.

Written 2026-11-27 after the 1.163 play test ("everything works perfectly fine").
Use the 1.151 test install at `AI funnies\151 Highfleet`; never the real Steam
folder.

## What the feature needs from the engine

Five things, in rough order of difficulty:

1. **Shop generation hook** тАФ to put the bar body into the city shop node with a
   stock count. qol already has a 1.151 hook for `shop_parts`; reuse it.
2. **`BottomMenu_` hover wrapper** тАФ the flyout is built from a hardcoded
   14-name chain. For a custom bar oid, rename the hovered body to `ITEM_HULL`
   for one call and point the hull palette global at the menu node.
3. **`ClickFunc` left-click guard** тАФ editor only. Rename the bar bodies to
   `ITEM_HULL` around the left-click call so the game's own 14-name guard fires.
4. **Flyout purchase path** тАФ the right-click buy calls
   `ObjectList_BuyPart -> _BuyPartShop`. The wrapper corrects the target node and
   provides the menu resource when the flyout lost the private stats.
5. **Private `Stats` records** тАФ one copy per menu part with
   `m_resources = [resource id; cost]`, appended to the game's `StatList`. This
   is what makes the shop display the real cost and lets the vanilla purchase
   path deduct the bar stock. A fallback that temporarily swaps the resource
   vector around the purchase call also exists in the 1.163 code; it can be used
   alone on 1.151 if the `StatList` append proves hard to port.

## 1.163 anchors тЖТ what to find in 1.151

The left column is the verified Steam 1.163 address used by
`src/part_menus.rs`. Find the 1.151 equivalent with Ghidra (program
`Highfleet.exe.unpacked.exe v1.51`). "known" means qol already has it.

| 1.163 | purpose | 1.151 |
|---|---|---|
| `0x1402bcd49`, 6 bytes | shop generation hook | **known** `0x140299c5c`, 6 bytes (`MOV ESI,[RAX+0x25c]`); qol `parts.rs` |
| `0x143a139f0` | `ALL_PARTS` pointer | **known** `0x143942568` |
| `0x14021b190` | `DefinePart(ALL_PARTS, oid, node, count)` | **known** `0x1401fde40` |
| `0x143a11940` | `CURRENT_LOCATION` (city) | **known** `0x1439220f0` |
| `+0x348` | city shop node | **known** `+0x348` |
| `+0x25c` | city type | **known** `+0x25c` |
| `0x14020d820` | `BottomMenu_(element, body)` | find by the 14-name chain; see below |
| `0x14014074a`, 5 bytes | `CALL BottomMenu_` in `mainMap` | find the only caller of `BottomMenu_` |
| `0x14012f120` | `ClickFunc(ctx, gui, button, value)` | find by the same 14-name chain / callers |
| `0x14012d168`, 5 bytes | `CALL ClickFunc` in `HandleClick` | find the caller of `ClickFunc` |
| `0x14016301f`, 5 bytes | `CALL _BuyPartShop` inside `ObjectList_BuyPart` | find `ObjectList_BuyPart` by the vtable slot that calls `_BuyPartShop` |
| `0x140221400` | `_BuyPartShop(part, count, target)` | find via `repairPartsStats` callers / the shop buy virtual |
| `0x1403bd400` | `EditorLibraryHull` palette node | find inside `DefineEditorParts` |
| `0x1403ef4f8` | `PARTS_CONSTRUCTOR_SERIA` | find inside `loadSeriaForLibraries_` |
| `0x1403ef36a` | `LOAD_CONSTRUCTOR_OR_EDITOR` | find inside `Scene_InitializeForMode`/`DefineEditorParts` |
| `0x143a13eb0` | `PLAYER_ESCADRA_` | find via `displayRepairMerchantCity` / the shop screen |
| `+0x520` | player part inventory node | find next to the shop list setup |
| `0x140334094` | `New`/`operator new` | already needed by other qol patches |
| `0x14027f200` | `Node::Node` | find by `New(0x128)` + name at `+0xC8` |
| `0x1400056e0` | `CopyString(dest, src, len)` | find by the same callers |
| `0x143a33bf0/bf8/c00` | `StatList` base/end/capacity vector | find by the resource lookup functions |
| `0x1402815d0` | `initStats1` (deep `Stats` copy) | find the stats copy path; may be named differently |
| `0x143a13f10`, stride `0x40` | runtime resource table | find by the resource-name getter (`0x140048c70` in 1.163) |
| `0x1403340d0` | CRT `free` matching `New` | find the `free` thunk used with `New` |

### Body / Stats / Node layouts to re-verify

Do not reuse the 1.163 offsets blindly. Check each against the 1.151 program and
against `hf_notes\docs\version-differences.md`:

- Body oid `+0x190`, length `+0x1a0`, capacity `+0x1a8`.
- Body count `+0x7a8`, `is_loot +0x7ac`, `is_blueprint +0x7ad`.
- Body stats index `+0x188`.
- `Stats` size `0x188` in 1.163; 1.151 is `0x180` and the fields after
  `m_sectors_type` shift by 8. The resource vector is documented as `+0x98`
  (begin), `+0xa0` (end), `+0xa8` (capacity) on both; verify.
- `Stats.m_price +0x88`.
- `Node` size `0x128`, child sentinel `+0x100`, entry next `+0x0`, payload
  `+0x10`, name `+0xC8`, child count `+0x108`.

### The 14-name chains

Both `BottomMenu_` and `ClickFunc` contain a chain of
`len == N && memcmp(oid, "ITEM_HULL", N)` comparisons. In Ghidra, search the
`ITEM_HULL` string (`0x14035ba98` in 1.163) and look at every xref; the same
function will reference `ITEM_ARMOR`, `ITEM_FUEL`, `ITEM_AMMO`, `ITEM_LEG`, тАж
The 1.151 string table will be at a different address but the chain shape is
identical. Rename the two functions `BottomMenu_` and `ClickFunc` and find their
call sites (the hover call is the only caller of `BottomMenu_`; the click call is
inside the GUI click dispatcher).

## Implementation order

1. **Create a 1.151 address module.** Split the `addresses` module in
   `src/part_menus.rs` into `#[cfg(feature = "1_151")]` and
   `#[cfg(not(feature = "1_151"))]` variants rather than the current single
   1.163 set.
2. **Reuse the existing 1.151 shop hook.** `parts.rs` already installs it and
   calls `part_menus::inject_menus`; remove the `#[cfg(not(feature = "1_151"))]`
   gate once the module builds for 1.151.
3. **Hooks, easiest first:**
   - click guard: find `ClickFunc`, verify the 14-name chain, hook its call
     site. Test in the editor with a `constructor.seria` button.
   - hover wrapper: find `BottomMenu_`, hook its call site. Test the flyout in
     the editor.
   - purchase wrapper: find `ObjectList_BuyPart`/`_BuyPartShop`, hook the call.
     Test buying a sub-part in a city shop.
4. **Private stats or temporary swap.** Try `initStats1` first; it is the clean
   solution (correct display, vanilla purchase). If the 1.151 stats copy or
   `StatList` append is unreliable, use only the temporary-swap path in `buy`
   (the 1.163 code already has it as a fallback) and accept that the flyout may
   display 0.
5. **Resource table.** Find the 1.151 equivalent of
   `0x140048c70`/`0x143a13f10`. Verify the table has room for ids `61..98`.
   If the 1.151 table is smaller, use existing unused ids and document the limit.
6. **Inventory menu.** Add `PLAYER_ESCADRA + inventory offset` to
   `bar_in_current_row` so a bought bar keeps its menu in the inventory.
7. **Prototype is_loot.** `mark_prototype_loot` is what stops a bought custom
   bar from floating when dragged onto a ship. Keep it; it edits only the
   configured bar oid's prototype in `ALL_PARTS`.
8. **Test in `151 Highfleet`.** Copy the DLL to
   `151 Highfleet\Modloader\mods\highfleet_qol.dll` (build with
   `cargo build --release --features 1_151`), add a demo `part_menus` entry,
   and check:
   - bar appears in a city shop with a stock;
   - hover opens the flyout;
   - right-click a sub-part consumes stock and gives the part;
   - a 2-unit part with 1 stock is refused cleanly;
   - the bought bar keeps its menu in the inventory;
   - dragging the bought bar onto the ship is refused, not floating;
   - the editor button still opens the menu and left-clicking it does not attach.

## Traps recorded from the 1.163 work

- **Right-click is the buy click.** The left-click guard must not rename the bar
  during a buy, or the bought bar changes oid (ITEM_PAPERS became HULL PARTS).
  In 1.163 the editor guard is `button == 1 && LOAD_CONSTRUCTOR_OR_EDITOR != 0`.
- **The flyout's target node is not always the city shop node.** Retry with
  `CURRENT_LOCATION + 0x348` only when the target does not already contain the
  bar; retrying unconditionally would let an inventory purchase fall through to
  the city bar.
- **`is_loot` is the difference between a resource bar and a placeable body.**
  Vanilla resource items have `is_loot=true` in `parts.seria`; most `ITEM_*`
  items do not. Set it on the clone and on the `ALL_PARTS` prototype.
- **A part needs a `Body` prototype, not just `OL.seria` stats.** The first demo
  used `MDL_BLOCK4X4_01`/`MDL_COMP2X2_01`, which have stats but no body in
  `parts.seria`; `DefinePart` returned null. Use an oid that exists in both
  files.
- **Private stats must be appended to the `StatList` vector with the game's
  allocator.** `New`/`free` are the CRT pair; use them for the vector and the
  copied record. Zero/copy the record with `initStats1`, never a raw memcpy.
- **Do not use the `E9`/`E8` bridge approach from the older MASM mods.** qol's
  `patchy` call-site patches already push a return address; a `jmp` replacement
  would crash.
- **ShipworksExtended coexistence:** it hooks the same `BottomMenu_`/`ClickFunc`
  call sites. qol loads first (`highfleet_qol.dll` before
  `ShipworksExtended.dll`), so qol owns them; ShipworksExtended logs a warning
  and its part loader still works.

## Open questions for 1.151

- Does 1.151 have the same two-node shop layout (`CURRENT_LOCATION + 0x348` and
  `PLAYER_ESCADRA + 0x520`), or only one?
- Is the resource table `id * 0x40` in 1.151, and how many entries does it have?
- Does 1.151's `BottomMenu_` guard skip the hull mapping in the shop like
  1.163's does? If not, the hover rename trick may behave differently.
- Does 1.151 rebuild `EditorLibraryHull` on every shop entry? If not, the
  augmentation must be added only once per node.
