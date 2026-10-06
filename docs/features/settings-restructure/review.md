# Review: Settings dialog restructure PRD (blast radius)

- Reviewed: `docs/features/settings-restructure/prd.md` (draft)
- Code read: `main` at `56c242a` (0.1.7) and the uncommitted scaling-fix work
  on `fix/scaling-filters-lanczos3-mitchell`
- Line numbers refer to `main` (`56c242a`) unless a line says
  "scaling branch". After the scaling fixes merge, the `src/main.rs` line
  numbers shift.
- No repo file was changed. All proofs ran on copies in `/tmp`
  (`/tmp/pelta-main`, `/tmp/pelta-wt`, `/tmp/pelta-merge`).

## 1. Verdict

The PRD is sound, and the processing code can stay untouched. The plan's
safety depends on one fact. I proved the processing half of it by running
code. The dialog half cannot be proven until the dialog exists (section 3).

Six things must change in the PRD before implementation starts:

1. The base fails both `cargo fmt --check` and
   `cargo clippy -- -D warnings`. `src/image_proc.rs` is not
   rustfmt-clean. Following the PRD's build checks literally would
   reformat image-processing code and break the hard constraint
   (risk R1).
2. Acceptance criterion 1 cannot protect the render path in
   `src/main.rs`, because the dialog also lives in that file (gap G1).
3. Three criteria cannot be observed as written: one rerender, no rerender
   when the dialog opens, and pixel-identical screenshots (section 7).
4. The QA items about the spinner are wrong. The spinner only shows when
   the picture has no texture yet (section 7).
5. The bottom-sheet threshold is 450 px wide or 360 px high, not "360 px"
   (gap G4).
6. Keyboard focus on radio rows is inconsistent by default. The decision
   is in section 10, D4.

## 2. What the change does, including what the diff will not show

- It replaces two dependent switches with one switch for one enum, and four
  exclusive switches with three radio rows plus an independent switch.
- **The behaviour change hidden in the layout:** Auto contrast stops being
  exclusive. Two value pairs that the UI could never produce become
  reachable: `(Lanczos3, true)` and `(MitchellNetravali, true)`. They reach
  `process_page` with no code change. The test in section 3 runs them.
- Opening the dialog sets the widgets' initial state, and that emits
  signals. `set_active` emits `toggled` (proven in section 3). If the
  handlers are connected first, simply opening the dialog writes settings
  and triggers a rerender.
- The scaling-fix branch adds a resize-driven rerender
  (`connect_slot_changed`, scaling branch `src/main.rs:630-660`). Any change
  to the reader's device-pixel size reprocesses pages. The dialog must not
  change that size. It does not (section 9).

## 3. The one fact the change is safe because of

> Rendered pixels are a pure function of (encoded page bytes, slot size in
> device pixels, `ImageProcessingSettings { scaling, auto_contrast }`,
> `ReadingMode`, `page-matte-detection`). The settings dialog is the only
> writer of those settings. It cannot change the slot size. So if the new
> dialog writes the same values as the old one, the pixels are the same.

| Part | Status | Evidence level |
|---|---|---|
| `process_page` is deterministic for all six `(scaling, auto_contrast)` pairs | **Proven** | 4: ran the real code |
| The six pairs give six different outputs, so the cache key must include `auto_contrast` (it does, `src/image_proc.rs:19-25`) | **Proven** | 4: ran the real code |
| The new pairs `(Lanczos3, true)` and `(MitchellNetravali, true)` run without error | **Proven** | 4: ran the real code |
| The dialog is the only writer of `reading.mode`, `image.scaling` and `image.auto_contrast` | Proven by search | 2: `src/main.rs:1008,1036,1129-1232`; no other assignments in `src/` |
| Opening the dialog cannot resize the reader | Proven from source | 2: `adw-dialog-host.c:332-337` (1.9.4) measures only the window content, never the dialog |
| The new dialog writes the same values for the four old selections | **Unproven** | 1: the dialog does not exist yet. The test is in section 11. |

Proof 1: the processing code (`/tmp/pelta-wt/src/blast_proof.rs`, which
calls the real `crate::image_proc::process_page` from the scaling branch on
`tests/fixtures/scaling/src/comic-halftone.png`):

```
running 1 test
Nothing auto_contrast=false -> 97x97 digest=3c99ba8cf0b68e04
Nothing auto_contrast=true -> 97x97 digest=8d6b8ba4484d2dd8
Lanczos3 auto_contrast=false -> 97x97 digest=0bc9b88b5117edd2
Lanczos3 auto_contrast=true -> 97x97 digest=2237dc390df773b1
MitchellNetravali auto_contrast=false -> 97x97 digest=f988c1d42dbb997f
MitchellNetravali auto_contrast=true -> 97x97 digest=2e1a657eed9c8dfc
test blast_proof::six_combinations_are_deterministic_and_distinct ... ok
```

Each pair is processed twice and the two results must be byte-identical.
All six digests must also be different from each other.

Proof 2: libadwaita behaviour (`/tmp/pelta-wt/examples/dialog_proof.rs`,
run against the host's libadwaita 1.9.4 and GTK 4.22.5, the same versions
as the GNOME 50 runtime):

```
libadwaita 1.9.4
activate Lanczos3 row        -> ["Default:false", "Lanczos3:true"]
activate Lanczos3 row again  -> []
set_active(Mitchell)         -> ["Lanczos3:false", "Mitchell:true"]
active flags                 -> [false, false, true]
view switcher mapped with 1 page -> false
search 'Two-page' (main page)      -> [... "[boxed-list] Two-page spreads" ...]
search 'Lanczos' (subpage not pushed) -> ["<stack:search>", "<stack:no-results>", ...]
search 'Lanczos' (subpage pushed)  -> ["[boxed-list] Lanczos3 (experimental)"]   (the subpage itself; the search view is hidden behind it)
Tab stops: GtkCheckButton(Default), AdwActionRow(Lanczos3 (experimental)), AdwActionRow(Mitchell-Netravali), AdwSwitchRow(Auto contrast), ...
```

What this shows:

- Selecting a row emits two `toggled` signals, old button first, then new.
  The PRD's rule "act only when the button becomes active" is correct and
  necessary.
- Activating the row that is already selected emits nothing.
- `set_active` emits `toggled`, so the initial state must be set before the
  handlers are connected, or behind a guard.
- With one page, the view switcher is not shown.
- Search finds main-page rows but not subpage rows. With the subpage open,
  the search view sits behind it.
- The example compiles against `libadwaita` 0.9 with feature `v1_9`, using
  `push_subpage`, `NavigationPage`, `ToolbarView`, `ActionRow::add_prefix`,
  `set_activatable_widget` and `CheckButton::set_group`.

## 4. Every place that reads or writes the settings the dialog touches

### 4.1 GSettings key `page-matte-detection` (the only key)

| Where | What |
|---|---|
| `data/com.pelta.ComicReader.gschema.xml:4-8` | Key definition, `type="b"`, default `true` |
| `data/meson.build` (`install_data` of the schema, `validate-schema` test) | Installs and validates the schema |
| `src/settings.rs:90` | `PAGE_MATTE_KEY` |
| `src/settings.rs:103` | `PAGE_MATTE_DEFAULT = true` (must match the schema) |
| `src/settings.rs:107-122` | `Preferences::load`: GSettings when the schema is installed, otherwise an in-memory session value |
| `src/settings.rs:128-133` | Read: `page_matte()` |
| `src/settings.rs:135-143` | Write: `set_page_matte()` |
| `src/settings.rs:194-240` | Tests: default, persistence, fallback |
| `src/reader_matte.rs:54-56` | `ReaderMatte::enabled()` reads the key |
| `src/reader_matte.rs:61-68` | `set_enabled()` writes the key. Turning it off clears the background at once. |
| `src/reader_matte.rs:79-82` | `show()` does nothing while the setting is off |
| `src/main.rs:384` | `ReaderMatte::new(&reader_overlay, Preferences::load(APP_ID))` |
| `src/main.rs:482-484`, `546-548` | `show_page` reads `matte.enabled()` |
| `src/main.rs:1058-1072` | Dialog: initial state and handler (calls `show_page` only when turned on) |

Outside the code: inside the Flatpak the value is stored in the app's
keyfile under `~/.var/app/com.pelta.ComicReader/`. That location is shared
by the system 0.1.7 install and a local `--user` build (section 8).

### 4.2 In-memory fields (not keys; reset at every launch)

| Field | Initialised | Written | Read |
|---|---|---|---|
| `reading.mode` | `src/main.rs:395-398` (`Default` = `SinglePage`) | `src/main.rs:1008`, `1036` (dialog only) | `src/main.rs:243` (async apply visibility), `424` (`show_page`), `620` (`go_prev`), `636` (`go_next`), `972` (dialog) |
| `image.scaling` | `src/main.rs:397` via `from_env` (`src/settings.rs:67-81`) | `src/main.rs:1129`, `1163`, `1197`, `1231` (dialog only) | `src/main.rs:425` (`show_page`, copied into `img_settings`), `1099` (dialog). Downstream: `schedule_processed_page` and `prefetch_processed_page` (`src/main.rs:180-297`), `CacheKey` (`src/image_proc.rs:19-25`), `process_page` (`src/image_proc.rs:97-121`) |
| `image.auto_contrast` | same as above | `src/main.rs:1130`, `1164`, `1198`, `1232` (dialog only) | same as `image.scaling` |
| Scaling branch extra reader | | | `connect_slot_changed` handler reads `settings.borrow().image.needs_processing()` (scaling branch `src/main.rs:~654`) |

Environment overrides: `PELTA_LANCZOS3`, `PELTA_MITCHELL`,
`PELTA_AUTO_CONTRAST` (`src/settings.rs:64-81`). The order is Lanczos3,
then Mitchell, then Auto contrast, and only one takes effect.

## 5. Every place the version string lives

| File:line (on `main`) | Value | Change needed |
|---|---|---|
| `Cargo.toml:3` | `version = "0.1.7"` | yes |
| `Cargo.lock:633-634` (`name = "pelta-linux-gnome"`) | `version = "0.1.7"` | yes, regenerated |
| `meson.build:4` | `version: '0.1.7'` | yes |
| `meson.build:24` | `version = meson.project_version()` | none. The variable is assigned but never used. |
| `data/com.pelta.ComicReader.metainfo.xml:42` | `<release version="0.1.7" date="2026-09-30">` | add a new `<release>` above it |
| `packaging/com.pelta.ComicReader.yml` | no version (`sources: type: dir, path: ..`) | none |
| About dialog / `CARGO_PKG_VERSION` | not present anywhere in `src/` | none |
| `.github/workflows/flatpak.yml` | no version; the release tag comes from GitHub | none (and nothing is pushed) |

Proven: `cargo update -p pelta-linux-gnome --offline` after the
`Cargo.toml` bump changes exactly one line of `Cargo.lock`:

```
Updating pelta-linux-gnome v0.1.7 (/tmp/pelta-main) -> v0.1.8
634c634
< version = "0.1.7"
---
> version = "0.1.8"
```

`cargo metadata --offline` gives the same one-line diff.

Proven: the PRD's draft `<release version="0.1.8">` entry passes
`appstreamcli validate --no-net --explain` (AppStream 1.2.1):
`✔ Validation was successful: pedantic: 1`. The same pedantic note exists
on `main` without the entry.

Version state on the scaling branch: `Cargo.toml`, `Cargo.lock` and
`meson.build` say 0.1.6, while its metainfo adds a 0.1.7 entry. Both
conflict with `main` (section 6, R5).

## 6. Risks (confirmed)

| # | How it breaks | Where | Likelihood | Cost | How to check |
|---|---|---|---|---|---|
| R1 | Running `cargo fmt` or `cargo clippy --fix` over the whole crate rewrites `src/image_proc.rs`, which breaks the hard constraint. The PRD's 11.4 and criterion 16 invite exactly that. | `cargo fmt --check` on `main` reports diffs in `src/image_proc.rs:194`, `src/main.rs:109`, `src/main.rs:437`. On the scaling branch `rustfmt --check` flags `src/image_proc.rs`. `cargo clippy --all-targets -- -D warnings` on `main` fails with 11 errors that existed before this work. | Medium | High (breaks the hard constraint) | The guard in section 11, step 1 |
| R2 | Extra rerenders, or a rerender when the dialog opens: the handler acts on the "deactivated" signal, or the handlers are connected before the initial state is set. | Proven signal behaviour (section 3, proof 2) | Medium | Low for the user (no visible flicker, because the old texture stays up), but it breaks criteria 7 and 11 | Unit test of the extracted handler logic (section 11) |
| R3 | An edit to the render path in `src/main.rs` (`show_page`, `schedule_processed_page`, `prefetch_processed_page`, `rerender`, the resize rerender) gets past criterion 1, because `src/main.rs` must change anyway. | `src/main.rs:180-297`, `418-613`, `979-992` | Low | High | A hunk-level guard on `src/main.rs` (G1) |
| R4 | Keyboard focus lands on the check button in one row and on the row itself in others, so Tab order and the focus ring are inconsistent. | Proof 2, "Tab stops" | High (it happened in the probe) | Low | Manual keyboard QA. Decision D4. |
| R5 | The base needs a merge first. Merging the scaling fixes into `main` conflicts in `src/main.rs` (only the `mod` and `use` lines: `page_picture` vs `matte` / `reader_matte`), `data/com.pelta.ComicReader.metainfo.xml` (two different 0.1.7 entries) and `README.md`. The dialog code merges cleanly (the probe merge kept both `matte_row` and the "Mitchell-Netravali" title). | `/tmp/pelta-merge` probe | Certain | Low | That resolution happens in the scaling-fix PR, and it decides 0.1.8 vs 0.1.9 |
| R6 | The new pairs (filter plus CLAHE) are slower per page than either step alone. The work runs on `gio::spawn_blocking` and the old texture stays up, so the user sees a delay, not a blank page. | `src/main.rs:216-217` | Low | Low | Manual QA on a large page |
| R7 | The local `--user` build and the system 0.1.7 install share one settings file, so turning page matte off in the test build also turns it off in 0.1.7. | `~/.var/app/com.pelta.ComicReader/` | Certain | Very low | Note it in QA |

## 7. Untestable or mis-specified criteria and QA items

| PRD item | Problem | Proposed fix |
|---|---|---|
| Criterion 7, "writes once and rerenders once" | Not visible by hand. The spinner does not show on a rerender, because `show_spinner = picture.paintable().is_none()` (`src/main.rs:209`; scaling branch `picture.texture().is_none()`, `src/main.rs:223`). | Move the "should this signal write?" decision into a pure function and unit-test it with the proven signal order. |
| Criterion 10, "pixel-identical to the base build" by screenshot | Fragile: compositor scaling, cursor, focus ring, async matte detection and timing all affect a screenshot. A difference proves nothing, and so does a match at only one size. | Accept the deterministic chain instead: the untouched-files guard, plus the value-mapping test, plus the `process_page` determinism proof. Keep screenshots as optional extra evidence only. |
| Criterion 11, "opening and closing causes no rerender" | Not visible, for the same reason as criterion 7 | Same seam and unit test, plus "initial state set before connect" visible in code review |
| Criterion 16, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` | Fails on the base before any change (R1) | Change to: no new clippy warnings compared with the base, `rustfmt --check` on new and changed files only, and never run `cargo fmt` over the whole crate |
| Section 11.4 | Same as criterion 16 | Same |
| QA "Each selection updates the open page at once (the spinner appears for filters)" | The spinner does not appear while a page is already shown | Change to: "the page visibly changes, within the processing time" |
| QA "Clicking the radio row that is already selected does nothing (no spinner)" | It can never show a spinner, so this checks nothing. The behaviour is proven at the toolkit level (proof 2). | Cover it with the unit test. Drop the manual item or reword it. |
| Criterion 14 and QA "At 360 px the dialog shows as a bottom sheet" | The threshold is `max-width: 450px or max-height: 360px` (`adw-dialog.c:1155`, 1.9.4). The window has no explicit minimum size (no `set_size_request` on the window), so whether it shrinks to 360 px depends on the header bar's minimum width. | Test at 450 px wide or 360 px high, whichever the window allows, and record the width actually reached |
| Criterion 13, keyboard table | Escape, Alt+Left and Back are proven only from source (`adw-navigation-view.c:2120-2126`) | Fine as manual QA. Keep it. |
| Criterion 17, "nothing pushed or sent to CI" | A process rule, not a product check | Check with `git status -sb` (not ahead of `origin`) and no new tags |

## 8. Flatpak manifest and runtime

| Item | Value |
|---|---|
| Manifest | `packaging/com.pelta.ComicReader.yml` |
| Runtime / SDK | `org.gnome.Platform` / `org.gnome.Sdk` `50` (`:17-19`) |
| SDK extensions | `rust-stable`, `llvm22` (`:20-22`). Installed for the user: branches 25.08 and 26.08. |
| libadwaita / GTK in SDK 50 | 1.9.4 / 4.22.5 (checked with `pkg-config` inside `org.gnome.Sdk//50`). The host has the same versions. |
| Build network | `build-args: --share=network` (`:33-34`). The local build downloads crates, so it needs internet. |
| Source | `type: dir, path: ..` |
| Wayland only | `finish-args` with no X11. The CI step `build-aux/check-flatpak-no-x11.sh` runs only on push or PR. |
| Installed now | `com.pelta.ComicReader 0.1.7`, system install, origin `comicreader1-origin` |
| CI triggers | push to `main`, `pull_request`, `release`, `workflow_dispatch`. A local build does not trigger CI. Pushing a branch alone does not either, but opening a PR does. |

## 9. Anything that could change rendered output, and its status

| Source | Affected by this feature? | Evidence |
|---|---|---|
| `process_page` and its resamplers (scaling branch `src/resample.rs`) and CLAHE | No. The files are untouched. | Criterion 1 guard; proof 1 |
| The `image.scaling` and `image.auto_contrast` values | Yes, through the new handlers. This is the one fact's unproven half. | Mapping test (section 11) |
| `reading.mode` | Yes, through the new switch | Mapping test |
| `page-matte-detection` and the matte colour | Handler moves as it is. Detection uses the raw encoded bytes, not processed pixels, so scaling and contrast cannot change it. | `src/reader_matte.rs:79-91` (`detect(&pages)` on the encoded bytes) |
| Slot size in device pixels (window and widget allocation, scale factor) | No. Presenting the dialog does not resize the window content. | `adw-dialog-host.c:332-337` |
| Process cache contents | The dialog calls `rerender`, which clears the cache and bumps `process_gen`. The cache key includes every image setting anyway. | `src/image_proc.rs:19-25`; `src/main.rs:979-992` |
| Resize-driven reprocess (scaling branch) | No, because the dialog does not change the slot size | as above |
| Colour scheme (`StyleManager::set_color_scheme`) | No | `src/main.rs:53`, not in the dialog |
| GTK texture scaling for "Default" with Auto contrast off | No. That is `page_picture` / `apply_raw_bytes`, which stay untouched. | Criterion 1 guard |
| `cargo fmt` or `clippy --fix` touching `src/image_proc.rs` | Formatting does not change pixels, but it breaks the "no diff" constraint | R1 |

## 10. Confirmation: the processing code can stay untouched

Yes. Every value pair the new UI can produce is already handled by
`process_page` (`src/image_proc.rs:106-121`): Nothing, Lanczos3 or Mitchell
resize, then optional CLAHE. Proof 1 ran all six pairs, including the two
new ones, against the real code. The settings types and `from_env` need no
change. The only allowed change in `src/settings.rs` is the stale doc
comment on `auto_contrast` (`src/settings.rs:53-54`, "Mutually exclusive
... in the UI").

Files that must show no diff against the base:
`src/image_proc.rs`, `src/resample.rs`, `src/page_picture.rs`,
`src/scaling_harness.rs`, `src/matte.rs`, `src/reader_matte.rs`,
`src/reading.rs`, `src/archive.rs`,
`data/com.pelta.ComicReader.gschema.xml`, `data/meson.build`, `tests/`,
`packaging/`, `.github/`. The last two are missing from the PRD's list.

## 11. Gaps in the PRD

- **G1. `src/main.rs` is not guarded.** The render path lives in the same
  file as the dialog. Add a hunk-level rule: every changed hunk in
  `src/main.rs` must fall inside `open_settings_dialog`, its call site, or
  the `mod` / `use` lines. That rule is easiest to check if the dialog moves
  to `src/settings_dialog.rs`, which leaves only deletions plus one call
  in `src/main.rs`.
- **G2. Format and lint policy is missing.** See R1 and section 7.
- **G3. The criterion 1 path list is incomplete.** Add `packaging/` and
  `.github/`.
- **G4. The bottom-sheet threshold is wrong.** It is 450 px wide or 360 px
  high, not 360 px.
- **G5. Focus on radio rows is not specified.** See R4 and D4.
- **G6. Shared settings with the system install are not mentioned in the
  QA setup.** See R7.
- **G7. Base preparation is a dependency, not a task.** The PRD says the
  scaling fixes land first. The conflict in R5 (two 0.1.7 metainfo entries)
  must be resolved there, and that result fixes this feature's version
  number.
- **G8. The summary row must update from the subpage handlers.** The
  handlers need a reference to the main-page row. This is trivial but not
  stated.

## 12. Cleared (checked and fine)

- **Search.** It is off. Ctrl+F does nothing, because `search_open_cb`
  returns early when search is disabled (`adw-preferences-dialog.c:504`).
  Subpage rows would not be found even with search on (proof 2).
- **View switcher.** It is hidden with one page (proof 2;
  `update_view_switcher`, `n_pages > 1`).
- **Radio exclusivity.** A `GtkCheckButton` group cannot reach "none
  selected" by user action. Re-activating the selected row emits nothing
  (proof 2).
- **Escape and back.** `AdwNavigationView` binds Escape, Back and Alt+Left
  to pop (`adw-navigation-view.c:2120-2126`). `AdwDialog` closes on Escape
  otherwise (`adw-dialog.c:633`). This is source-level only; confirm in QA.
- **APIs.** Every API in PRD section 5.4 compiles and runs with
  `libadwaita` 0.9 (`v1_9`) against 1.9.4 (proof 2).
- **No other writers.** No keyboard shortcut, menu or action writes the
  reading or image settings. The only writes are in the dialog.
- **Env overrides.** `from_env` is unchanged. Its exclusive order still
  decides the startup state, and the PRD documents this correctly.
- **Version tooling.** The `Cargo.lock` command and the metainfo entry
  are proven (section 5). The meson `version` variable is unused. There is
  no About dialog and no version in the manifest.
- **Matte independence.** Matte detection reads the encoded bytes, so the
  new filter-plus-contrast pairs cannot change the background colour.

## 13. What you must decide before implementation

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D1 | Scaling-fix release | Its version (0.1.8 or another number), and how it resolves the two 0.1.7 metainfo entries and the `README.md` conflict. That fixes whether this feature is 0.1.8 or 0.1.9. | Decide in the scaling-fix PR before branching |
| D2 | Format and lint policy | (a) only new and changed code must be rustfmt-clean, with no new clippy warnings compared with the base, and never run `cargo fmt` over the whole crate; (b) fix the baseline first in a separate PR, which touches `src/image_proc.rs` | (a). If you want (b), do it as its own PR before this one. |
| D3 | Where the dialog code lives | (a) move it to `src/settings_dialog.rs` (larger diff, but `src/main.rs` keeps only deletions plus a call, which is easy to guard); (b) edit it in place in `src/main.rs` | (a) |
| D4 | Keyboard focus on radio rows | (a) libadwaita default: focus sometimes lands on the check button and sometimes on the row; (b) make the check buttons non-focusable so focus always lands on the row, then confirm Space and Enter still select | (b), confirmed by manual keyboard QA. It is not a rendering change. |
| D5 | Render-equivalence evidence | (a) the deterministic chain (guard, mapping test, `process_page` determinism); (b) also require screenshot comparison | (a), with screenshots optional |
| D6 | "Default" subtitle | Keep "GTK scales the page." (inexact when Auto contrast is on), or reword it | Your call. The PRD keeps the brief's wording. |
| D7 | Handler test seam | Accept a small pure function for "should this signal write, and what value", so criteria 7 and 11 can be tested | Accept |
| D8 | Local QA environment | Accept the shared settings file with the system install, or uninstall or ignore the 0.1.7 install while testing | Accept, and note it in QA |

## 14. Before you merge: the cheapest checks that catch the real bugs

1. **Diff guard** (catches R1 and R3):

   ```bash
   cd /home/lozbek/code/pelta-cbr-linux
   git diff --stat <base>...HEAD -- src/image_proc.rs src/resample.rs \
     src/page_picture.rs src/scaling_harness.rs src/matte.rs src/reader_matte.rs \
     src/reading.rs src/archive.rs data/com.pelta.ComicReader.gschema.xml \
     data/meson.build tests packaging .github
   # expected: no output
   git diff -U0 <base>...HEAD -- src/main.rs   # every hunk inside the dialog, its call site, or mod/use lines
   git diff <base>...HEAD -- src/settings.rs   # only the auto_contrast doc comment
   ```

2. **Mapping test** (the unproven half of the one fact). Old dialog writes
   on the left, new controls on the right. It must pass in `cargo test`:

   ```rust
   // old selection          -> (scaling, auto_contrast) written by the old dialog
   // Nothing                -> (Nothing, false)
   // Lanczos3               -> (Lanczos3, false)
   // Mitchell-Netravali     -> (MitchellNetravali, false)
   // Auto contrast          -> (Nothing, true)
   // The new dialog must write the same pairs for:
   // Default+off, Lanczos3+off, Mitchell+off, Default+Auto contrast on.
   // Reading: Single-page on -> SinglePage; Two-page on -> TwoPageSpreads,
   // equal to switch off / switch on.
   ```

3. **Handler seam test** (catches R2). Feed the proven signal order
   `[("Default", false), ("Lanczos3", true)]` and assert exactly one write
   of `Lanczos3`. Feed `[]` (activating the selected row again) and assert
   no write. Assert that setting the initial state writes nothing.

4. **Processing determinism** (keep it as a regression test if you like):
   `/tmp/pelta-wt/src/blast_proof.rs`, shown in section 3. It runs in
   0.02 s.

5. **libadwaita probe** (rerun if the runtime changes):
   `/tmp/pelta-wt/examples/dialog_proof.rs`, with
   `CARGO_TARGET_DIR=/tmp/pelta-target cargo run --release --example dialog_proof`
   in a Wayland session.

6. **Release checks:** `appstreamcli validate --no-net --explain
   data/com.pelta.ComicReader.metainfo.xml`, then the local Flatpak build
   (PRD 8.3) with `flatpak run --user com.pelta.ComicReader`.
