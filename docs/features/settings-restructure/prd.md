# PRD: Settings dialog restructure

- Status: draft, waiting for approval
- Target version: 0.1.8 (see "Version" for the collision rule)
- Platform: Rust, GTK 4.22.5, libadwaita 1.9.4 (GNOME 50 runtime), Flatpak
- Base: a new branch from `main`, cut after the scaling-fix branch
  (`fix/scaling-filters-lanczos3-mitchell`) has merged into `main`

## 1. Summary

Rebuild the Settings dialog so it is easier to use. The dialog gets one main
page with no view switcher, and image processing moves to a subpage. Only the
libadwaita widget tree and the code that connects the widgets to the existing
settings change. Image processing, the reader, the GSettings schema and the
settings data types stay as they are. For the same settings, the reader
shows the same pixels as before.

## 2. Findings that shaped this PRD

These facts come from the code on `main` (0.1.7) and the libadwaita 1.9.4
source. They correct some assumptions in the original brief.

1. **Only one setting is a GSettings key.** The schema
   `data/com.pelta.ComicReader.gschema.xml` has one key:

   | Key | Type | Default | Meaning |
   |---|---|---|---|
   | `page-matte-detection` | `b` | `true` | Match page border colour |

   All other settings are in-memory fields in `src/settings.rs`. They reset
   at every launch:

   | Setting | Field | Type | Default |
   |---|---|---|---|
   | Reading mode | `AppSettings.reading.mode` | `ReadingMode { SinglePage, TwoPageSpreads }` | `SinglePage` |
   | Scaling | `AppSettings.image.scaling` | `ScalingMode { Nothing, Lanczos3, MitchellNetravali }` | `Nothing` |
   | Auto contrast (CLAHE) | `AppSettings.image.auto_contrast` | `bool` | `false` |

   Soak-test environment variables preselect the image settings at startup
   (`ImageProcessingSettings::from_env`). If more than one is set, the order
   is `PELTA_LANCZOS3`, then `PELTA_MITCHELL`, then `PELTA_AUTO_CONTRAST`,
   and only one of them takes effect.
2. **"Single-page" and "Two-page spreads" are not two keys.** They are two
   switches for one enum. The dialog code keeps exactly one of them on.
3. **Today Auto contrast excludes the scaling filters.** The current dialog
   shows four switches (Nothing, Lanczos3, Mitchell, Auto contrast) and
   exactly one of them is on. `process_page` already supports a scaling
   filter followed by CLAHE. The UI just cannot select that combination yet.
4. **The settings dialog is the only code that writes these fields.**
   No keyboard shortcut or menu changes them.
5. **Dialog search does not index subpages.** In libadwaita 1.9.4,
   `AdwPreferencesDialog` builds its search model from
   `adw_preferences_page_get_rows()` on the pages added with
   `adw_preferences_dialog_add()` only (`adw-preferences-dialog.c`,
   `preferences_page_to_rows`). Rows in a page shown with `push_subpage()`
   are not found. Search is off in the app today
   (`set_search_enabled(false)`).
6. **The view switcher appears only with more than one page**
   (`update_view_switcher`: `n_pages > 1`). One page means no view switcher.
7. **Escape goes back from a subpage.** `AdwNavigationView` binds Escape,
   the Back key, Alt+Left and the back mouse button to "pop"
   (`adw-navigation-view.c`). `AdwDialog` closes on Escape when nothing
   else handles it.
8. **There is no About dialog**, and the Flatpak manifest has no version
   (it builds from `type: dir`).

## 3. Decisions (from the interview)

| # | Topic | Decision |
|---|---|---|
| 1 | Base | New branch from `main`. The scaling fixes land on `main` first, in their own PR. |
| 2 | Version | This feature is 0.1.8, not 0.8. |
| 3 | Persistence | Reading mode, scaling and Auto contrast stay in-memory, as today. No schema change, no new keys. |
| 4 | Auto contrast | Becomes an independent switch. Combinations such as Lanczos3 with Auto contrast become selectable. |
| 5 | Mitchell label | "Mitchell-Netravali" (the kernel is Mitchell, B = C = 1/3; it never uses Catmull-Rom). |
| 6 | Search | Stays disabled, as today. |
| 7 | Release notes | The scaling-fix PR handles its own release entry. The 0.1.8 entry describes only the Settings layout. |
| 8 | Version collision | This feature is always the next patch version after `main` at branch time: 0.1.8, or 0.1.9 if the scaling-fix PR took 0.1.8. |

## 4. Goals and non-goals

### Goals

- One main page with short, clear groups. No view switcher, no page icons.
- One switch for the reading mode, not two switches that depend on each other.
- Scaling as a real single choice, with radio-style rows.
- A navigation row that shows the current image-processing choice.
- The open page updates immediately on every change, with no restart.

### Non-goals

- No change to image processing, scaling kernels, CLAHE, page matte
  detection, the reader, `page_picture` or the prefetch cache.
- No GSettings schema change, no new keys, no migration, no persistence for
  the in-memory settings.
- No "Advanced" page, no new settings, no new filters.
- No dialog search.
- No About dialog.
- No push, tag, GitHub release or CI run. The Flatpak is built locally only.

## 5. Target UI

```
Settings                                   (AdwPreferencesDialog, one page)
├─ Reading
│   └─ [switch] Two-page spreads
│        Facing pages; cover and back cover stay alone.
├─ Appearance
│   └─ [switch] Match page border colour
│        Use each page's border colour as the reader background.
└─ (group with no title)
    └─ Image processing                                         >
         <summary, for example "Lanczos3, Auto contrast">

Image processing                           (subpage, back button in header)
├─ Scaling
│   ├─ (o) Default
│   │       GTK scales the page.
│   ├─ ( ) Lanczos3 (experimental)
│   │       <current Lanczos3 subtitle, unchanged>
│   └─ ( ) Mitchell-Netravali
│           <current Mitchell subtitle, unchanged>
└─ Tone
    └─ [switch] Auto contrast
         <current Auto contrast subtitle, unchanged>
```

### 5.1 Main page

- `AdwPreferencesDialog`, title "Settings", `search-enabled = false`.
- One `AdwPreferencesPage`. Its title stays "General". It has no icon. With
  one page the title is not shown, but libadwaita uses it internally.
- **Reading**: one `AdwSwitchRow`, "Two-page spreads", subtitle
  "Facing pages; cover and back cover stay alone." On means
  `ReadingMode::TwoPageSpreads`, off means `ReadingMode::SinglePage`.
- **Appearance**: the existing "Match page border colour" `AdwSwitchRow`,
  moved as it is. Title, subtitle and handler do not change.
- **Image processing**: an `AdwActionRow` with `activatable = true`, a
  `go-next-symbolic` suffix icon and the summary as its subtitle.
  Activating it calls `dialog.push_subpage(&subpage)`. It sits in its own
  `AdwPreferencesGroup` with no title, so the page does not show
  "Image processing" twice.

### 5.2 Subpage

- An `AdwNavigationPage` with title "Image processing". Its child is an
  `AdwToolbarView` with an `AdwHeaderBar` as the top bar (this gives the
  back button) and an `AdwPreferencesPage` as the content.
- **Scaling** group: three `AdwActionRow`s. Each row has a `GtkCheckButton`
  as its prefix and sets that button as its `activatable-widget`, so a click
  anywhere on the row selects it. All three check buttons share one group
  (`set_group`), so GTK draws them as radio buttons and keeps exactly one
  active.
  - "Default", subtitle "GTK scales the page.", maps to `ScalingMode::Nothing`.
  - "Lanczos3 (experimental)", maps to `ScalingMode::Lanczos3`.
  - "Mitchell-Netravali", maps to `ScalingMode::MitchellNetravali`.
  - The Lanczos3 and Mitchell rows keep their subtitles from the base
    branch, as they are after the scaling-fix PR.
- **Tone** group: one `AdwSwitchRow`, "Auto contrast". It keeps its subtitle
  from the base branch.

### 5.3 Summary text on the "Image processing" row

Format: `<scaling name>`, followed by `, Auto contrast` when Auto contrast is on.

| Scaling | Auto contrast | Summary |
|---|---|---|
| Default | off | Default |
| Default | on | Default, Auto contrast |
| Lanczos3 | off | Lanczos3 |
| Lanczos3 | on | Lanczos3, Auto contrast |
| Mitchell-Netravali | off | Mitchell-Netravali |
| Mitchell-Netravali | on | Mitchell-Netravali, Auto contrast |

"(experimental)" is not part of the summary. The summary updates as soon as
a subpage control changes, before the user goes back.

### 5.4 APIs and minimum versions

Every API below exists in libadwaita 1.9.4 and GTK 4.22.5. The crate already
enables `libadwaita` feature `v1_9` and `gtk4` feature `gnome_50`.

| API | Since |
|---|---|
| `AdwPreferencesDialog`, `push_subpage`, `pop_subpage`, `search-enabled` | 1.5 |
| `AdwNavigationPage`, `AdwToolbarView`, `AdwSwitchRow` | 1.4 |
| `AdwActionRow` `activatable-widget`, `add_prefix`, `add_suffix` | 1.0 |
| `GtkCheckButton` `set_group` (radio look) | GTK 4.0 |

## 6. Behaviour rules

### 6.1 Writing settings

| Control | Writes | Then |
|---|---|---|
| Two-page spreads switch | `reading.mode` | `show_page(current_page)` if an archive is open. The process cache is not cleared. This is what the current code does. |
| Match page border colour | `ReaderMatte::set_enabled` (key `page-matte-detection`) | The current handler, unchanged: `show_page` only when the switch turns on. |
| Scaling radio row | `image.scaling` only | The existing `rerender`: clear the process cache, bump `process_gen`, `show_page(current_page)`. |
| Auto contrast switch | `image.auto_contrast` only | The existing `rerender`. |

- **Exactly one scaling choice.** `image.scaling` is an enum, so one
  assignment changes the choice in one step. The previous choice cannot stay
  on as well.
- **One rerender per user action.** When the user selects a new radio row,
  GTK sends two `toggled` signals: one for the old button and one for the
  new one. The handler acts only on the signal where the button becomes
  active. It ignores the other signal and does nothing when the value has
  not changed.
- **Opening the dialog does not write.** Widgets are given their initial
  state from `AppSettings` before the handlers are connected, or a guard
  blocks the handlers while that happens. Opening and closing the dialog
  without a change causes no rerender.
- **Changes do not touch other settings.** Selecting a scaling row does not
  change Auto contrast. Toggling Auto contrast does not change scaling.
  This is the intended change from decision 4.

### 6.2 Precedence when the input has more than one filter

The in-memory model cannot hold two scaling filters at once, so the dialog
needs no precedence of its own. It shows `image.scaling` and
`image.auto_contrast` exactly as they are. The only input with more than one
filter is the environment, and the existing `from_env` resolves it in the
order Lanczos3, then Mitchell, then Auto contrast, with only one of them
enabled. That code does not change. A user who starts with
`PELTA_LANCZOS3=1 PELTA_AUTO_CONTRAST=1` gets Lanczos3 with Auto contrast off,
as today, and can then turn Auto contrast on in the dialog.

### 6.3 Render equivalence

For every combination that the old dialog could select, the new dialog writes
the same `ImageProcessingSettings` and `ReadingMode` values. Because
`process_page`, the resamplers, CLAHE, matte detection and `page_picture` do
not change, the rendered pixels are the same:

| Old dialog selection | New dialog selection | `scaling` | `auto_contrast` |
|---|---|---|---|
| Nothing (default) | Default, Auto contrast off | `Nothing` | `false` |
| Lanczos3 | Lanczos3, Auto contrast off | `Lanczos3` | `false` |
| Mitchell-Netravali | Mitchell-Netravali, Auto contrast off | `MitchellNetravali` | `false` |
| Auto contrast | Default, Auto contrast on | `Nothing` | `true` |
| (not selectable) | Lanczos3, Auto contrast on | `Lanczos3` | `true` |
| (not selectable) | Mitchell-Netravali, Auto contrast on | `MitchellNetravali` | `true` |

| Old reading switches | New switch | `reading.mode` |
|---|---|---|
| Single-page on | Two-page spreads off | `SinglePage` |
| Two-page spreads on | Two-page spreads on | `TwoPageSpreads` |

Note on wording: with "Default" and Auto contrast on, `process_page` resizes
with the `image` crate's Triangle filter before CLAHE, so GTK does not scale
the page in that one case. The brief fixed the "GTK scales the page."
subtitle. This PRD keeps it and records the nuance here. Changing the code
to match the subtitle is out of scope.

## 7. Search, narrow widths and keyboard

### 7.1 Search

Search stays disabled. Section 2, finding 5 shows that rows in a subpage
cannot be found. With six rows in total, search adds little. If search is
needed later, the least bad option is to enable it and accept that only
main-page rows match. The "Image processing" row matches on its summary text.

### 7.2 Narrow widths

`AdwDialog` turns into a bottom sheet when the window is too small. Long
titles and subtitles in rows wrap. Check at the GNOME minimum width of
360 px (manual QA). Nothing may be clipped, the chevron stays visible and the
back button stays reachable.

### 7.3 Keyboard

The expected behaviour comes from the libadwaita source. Manual QA must
confirm it on the real build.

| Key | Where | Expected |
|---|---|---|
| Tab / Shift+Tab, Up / Down | both pages | Move focus between rows and the back button |
| Space or Enter | switch row | Toggle the switch |
| Enter | "Image processing" row | Open the subpage |
| Space or Enter | scaling row | Select that row |
| Escape | subpage | Go back to the main page |
| Alt+Left | subpage | Go back to the main page |
| Escape | main page | Close the dialog |

After going back, focus should return to the "Image processing" row.
Record what actually happens. If focus goes somewhere else, write it down.
Do not work around it in this feature.

## 8. Version and release

### 8.1 Where the version lives (on `main`, now 0.1.7)

| File | Field | Change |
|---|---|---|
| `Cargo.toml` | `[package] version` | set to `0.1.8` |
| `Cargo.lock` | `[[package]] name = "pelta-linux-gnome"` `version` | regenerate with `cargo update -p pelta-linux-gnome --offline` (or any cargo build); no other lock changes |
| `meson.build` | `project(... version: ...)` | set to `'0.1.8'` |
| `data/com.pelta.ComicReader.metainfo.xml` | new `<release>` at the top of `<releases>` | add `0.1.8` with the build date |
| `packaging/com.pelta.ComicReader.yml` | none (`type: dir`) | no change |
| About dialog | does not exist | no change |

If the scaling-fix PR has already taken 0.1.8 when the branch is cut, use
0.1.9 everywhere in this table.

### 8.2 Release note (metainfo)

```xml
<release version="0.1.8" date="YYYY-MM-DD">
  <description>
    <p>Settings are simpler. Turn two-page spreads on or off with one switch. Scaling and Auto contrast are now on their own "Image processing" page, and you can use Auto contrast together with a scaling filter.</p>
  </description>
</release>
```

### 8.3 Local Flatpak build (no push, no tag, no CI)

```bash
cd /home/lozbek/code/pelta-cbr-linux
flatpak-builder --user --install --install-deps-from=flathub --force-clean \
  build-dir packaging/com.pelta.ComicReader.yml
flatpak run --user com.pelta.ComicReader
```

A system install of 0.1.7 from `comicreader1-origin` exists. Use `--user` so
the test runs the local build. The CI workflow runs on pushes to `main` and on
pull requests, so nothing is pushed and no PR is opened as part of this work.

## 9. Acceptance criteria

1. **No image-processing or schema diff.** Run
   `git diff --stat <base>...HEAD` on these paths and get no output:
   `src/image_proc.rs`, `src/resample.rs`, `src/page_picture.rs`,
   `src/scaling_harness.rs`, `src/matte.rs`, `src/reader_matte.rs`,
   `src/reading.rs`, `src/archive.rs`, `data/com.pelta.ComicReader.gschema.xml`,
   `data/meson.build`, `tests/`.
   In `src/settings.rs`, the types, defaults, `from_env`, `Preferences` and
   `PAGE_MATTE_KEY` do not change. The only change allowed there is the
   stale doc comment on `auto_contrast`, which says it is exclusive in the UI.
2. The dialog has one page. No view switcher and no page icon appear at any
   width.
3. The main page shows exactly three groups in this order: Reading
   (one switch), Appearance (one switch), and the "Image processing"
   navigation row with a chevron and the summary from section 5.3.
4. "Two-page spreads" on gives `TwoPageSpreads`, and off gives `SinglePage`.
   The open comic changes layout immediately, as it did with the old
   switches.
5. "Match page border colour" behaves exactly as before and still writes the
   `page-matte-detection` key. The value survives a restart.
6. Activating "Image processing" opens the subpage with the Scaling group
   (three radio rows) and the Tone group (Auto contrast).
7. Exactly one scaling row is selected at all times. A click anywhere on a
   row selects it. Each selection writes `image.scaling` once and rerenders
   once.
8. Auto contrast toggles on its own. It does not change the scaling choice,
   and the scaling choice does not change it.
9. Every change updates the open page immediately, with no restart. The
   summary updates before the user goes back.
10. For each of the four old selections, the new dialog writes the same
    values (section 6.3). Rendered pages are pixel-identical to the base
    build.
11. Opening and closing the dialog without a change causes no rerender and
    changes no setting.
12. Search stays disabled. Ctrl+F does nothing in the dialog.
13. The keyboard table in section 7.3 holds. Any difference is recorded in
    the QA notes.
14. At a width of 360 px nothing is clipped, and the subpage and back button
    work.
15. The version is 0.1.8 (or 0.1.9, as in section 8.1) in `Cargo.toml`,
    `Cargo.lock`, `meson.build` and a new metainfo `<release>`.
    `appstreamcli validate --no-net` passes.
16. `cargo test`, `cargo clippy --all-targets` and `meson test` (schema,
    desktop and AppStream validation) pass.
17. The local Flatpak builds, installs with `--user` and runs. Nothing is
    pushed, tagged, released or sent to CI.

## 10. Edge cases

| Case | Expected |
|---|---|
| No archive open | Every control still writes its setting. Nothing renders. The next opened comic uses the new settings. |
| Change while a page is processing (spinner visible) | The existing `process_gen` bump drops the old result. The page renders once with the new settings. |
| Several quick radio changes | Each change bumps `process_gen`. Only the last one shows on screen. |
| Click the radio row that is already selected | No write and no rerender. |
| Started with `PELTA_LANCZOS3=1 PELTA_AUTO_CONTRAST=1` | Dialog shows Lanczos3 with Auto contrast off (existing `from_env` order). The summary is "Lanczos3". |
| Started with `PELTA_AUTO_CONTRAST=1` | Dialog shows Default with Auto contrast on. The summary is "Default, Auto contrast". |
| GSettings schema not installed (`cargo run`) | Page matte uses its session fallback, as today. The other settings are unaffected. |
| Dialog closed while the subpage is open | The next open starts on the main page with current values. |
| Two-page spreads on a comic with an odd page count, or on the cover | Same layout rules as before (`view_for_page` is unchanged). |
| Two-page spreads with a filter or Auto contrast on | Both pages are processed as before. |
| Window resized while the dialog is open | The dialog adapts. The settings stay as they are. |

## 11. Test plan

### 11.1 Automated (`cargo test`)

Move the pure mapping logic out of the GTK callbacks so it can be tested
without a display. A new module `src/settings_dialog.rs` holds the dialog
and these helpers:

- `image_summary(ImageProcessingSettings) -> String`: test all six rows of
  the table in section 5.3.
- `reading_mode_from_switch(bool) -> ReadingMode` and back: test both
  directions.
- An equivalence test: for each of the four old dialog selections, the
  values the new controls write equal the values the old dialog wrote
  (section 6.3).

Existing tests (settings persistence, matte, scaling harness) run unchanged
and pass.

### 11.2 Diff guard

Run the command from acceptance criterion 1 and attach the empty output to
the review.

### 11.3 Render equivalence

Image code does not change, so equivalence follows from criterion 1 and the
mapping test. As extra evidence, open the same page in the base build and in
the new build at the same window size and scale. Take screenshots for each
of the four old selections and compare them, for example with
`magick compare -metric AE` (expected difference: 0).

### 11.4 Build checks

`cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`,
`meson test -C <builddir>` and the local Flatpak build from section 8.3.

## 12. Manual QA checklist

Run on the local Flatpak (`flatpak run --user com.pelta.ComicReader`),
with a multi-page CBZ open.

**Structure**
- [ ] The dialog title is "Settings". There is no view switcher, no page icon and no search button.
- [ ] The groups appear in this order: Reading, Appearance, then the Image processing row.
- [ ] The Image processing row shows a chevron and the correct summary.

**Reading**
- [ ] Two-page spreads on: spreads appear at once, and the cover and back cover stay alone.
- [ ] Two-page spreads off: single page at once, on the same page.

**Appearance**
- [ ] Match page border colour off and on: the background changes as before.
- [ ] Restart the app: the matte setting is kept. Reading mode, scaling and Auto contrast are back to their defaults (same as 0.1.7).

**Image processing subpage**
- [ ] Clicking the row, or pressing Enter on it, opens the subpage with a back button.
- [ ] Default, Lanczos3 and Mitchell-Netravali can each be selected by clicking the title, the subtitle or the radio button. Only one is selected at any time.
- [ ] Each selection updates the open page at once (the spinner appears for filters).
- [ ] Auto contrast on and off works with each of the three scaling choices, and the page updates at once.
- [ ] The summary on the main page matches the subpage after going back.
- [ ] Clicking the radio row that is already selected does nothing (no spinner).

**Equivalence**
- [ ] Screenshots for the four old selections match the base build (section 11.3).

**Keyboard only (no mouse)**
- [ ] Tab reaches every row on both pages.
- [ ] Space or Enter toggles the switches and selects the radio rows.
- [ ] Enter on Image processing opens the subpage.
- [ ] Escape in the subpage goes back, and Escape on the main page closes the dialog.
- [ ] Alt+Left in the subpage goes back.
- [ ] Note where focus goes after going back.

**Narrow width**
- [ ] At a window width of 360 px the dialog shows as a bottom sheet, nothing is clipped, text wraps, and the chevron and back button are visible.
- [ ] The subpage works at 360 px.

**Startup overrides**
- [ ] `PELTA_LANCZOS3=1 PELTA_AUTO_CONTRAST=1`: the dialog shows Lanczos3 with Auto contrast off.
- [ ] `PELTA_AUTO_CONTRAST=1`: the dialog shows Default with Auto contrast on.

**Release**
- [ ] The installed Flatpak reports version 0.1.8 (`flatpak info --user com.pelta.ComicReader`).
- [ ] Nothing was pushed, tagged or released.

## 13. Dependencies and risks

- **The scaling-fix PR must merge into `main` first.** Its subtitles and
  "Mitchell-Netravali" label are the base for this feature. Its version
  decides whether this feature is 0.1.8 or 0.1.9.
- **New combinations.** Lanczos3 or Mitchell with Auto contrast become
  selectable. They use existing code paths (resize, then CLAHE), but users
  could not reach them before. Cover them in manual QA.
- **The "GTK scales the page." subtitle is not exact** when Auto contrast is
  on (section 6.3 note). The brief set this wording on purpose.
- **Focus after going back** depends on libadwaita. The behaviour is
  recorded in QA and not changed in this feature.
