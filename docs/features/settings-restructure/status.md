# Status: Settings dialog restructure

Branch `feat/settings-restructure`, cut from `main` at `56c242a` (0.1.7).
The spec is `prd.md`; the risks are in `review.md`. The decisions below
override both where they differ.

## Decisions in force

- Base is today's `main`. The scaling fixes are not in it, so the Lanczos3,
  Mitchell and Auto contrast row titles and subtitles stay exactly as on
  `main`: "Lanczos3 (experimental)" / "Linear-light Lanczos3 resize to
  display size", "Mitchell-Netravali / Catmull-Rom" / "Linear-light Mitchell
  resize to display size", "Auto contrast" / "CLAHE on luminance
  (tile-based, clip-limited)".
- The summary on the "Image processing" row uses the short names from PRD
  5.3: "Default", "Lanczos3", "Mitchell-Netravali", plus ", Auto contrast"
  when it is on.
- Version is 0.1.8.
- The dialog moves out of `src/main.rs` into `src/settings_dialog.rs`.
- Radio check buttons do not take keyboard focus. Focus lands on the row,
  and Space or Enter on the row selects it.
- No screenshot comparison.
- The Default row keeps the subtitle "GTK scales the page."
- A small pure-function seam makes "writes once, no write on open, no write
  on re-click" unit-testable.
- Lint and format: only new and changed code must be rustfmt-clean, and
  there must be no new clippy warnings compared with `56c242a`. Never run
  `cargo fmt` on the crate or `cargo clippy --fix`.
- No new dependencies. No GSettings schema change. Reading mode, scaling and
  Auto contrast stay in memory.

## Pre-existing condition found

`matte::tests::detection_is_fast_on_a_full_size_page` asserts a median
under 5 ms. In a debug build it is flaky under machine load: it failed once
on the untouched base `56c242a` (median 8.5 ms) and once on this branch
while other builds ran, then passed on a quiet rerun. It always passes with
`cargo test --release`, which is what `meson test` runs in the Flatpak. Each
step below therefore records both a debug and a release test run.

## Step 1: pure mapping logic (`15e1d5a`)

- State: done.
- Changed: new `src/settings_dialog.rs` (GTK-free helpers and 9 unit
  tests), `src/main.rs` (one line: `mod settings_dialog;`).
- Helpers: `reading_mode_from_switch`, `switch_from_reading_mode`,
  `reading_write`, `scaling_write`, `contrast_write`, `scaling_name`,
  `image_summary`. A temporary `#![cfg_attr(not(test), allow(dead_code))]`
  stays until step 3 uses them.
- Checks: `cargo build` ok. `cargo test` 46 passed, 1 ignored (debug and
  release), including 9 new `settings_dialog` tests. Guard output empty:
  yes; the only `src/main.rs` hunk is the `mod` line. Clippy new warnings:
  no (identical to base, nothing in `settings_dialog.rs`). `rustfmt --check`
  on `settings_dialog.rs`: clean.
- Deviations from the PRD: none.
- Open items for manual QA: none yet.

## Step 2: main page (`e5a819c`)

- State: done.
- Changed: `open_settings_dialog` moved from `src/main.rs` into
  `src/settings_dialog.rs` as `pub fn` with the same parameters. In
  `src/main.rs` the only hunks are the `mod` line, the `use` lines
  (`ReadingMode` and `ScalingMode` no longer imported; the function is
  imported by name so the call site in `settings_btn.connect_clicked` is
  unchanged) and the removed function with its doc comment.
- Main page: no page icon, page title "General", dialog title "Settings",
  search off. Reading is one switch, "Two-page spreads", subtitle "Facing
  pages; cover and back cover stay alone." Its initial state is set before
  the handler is connected; the handler uses `reading_write` and, on a
  change, sets `reading.mode` and calls `show_page(current_page)` when an
  archive is open (no cache clear, as before). Appearance is unchanged. The
  old Image processing group (four exclusive switches) is kept as it was so
  the app works between commits.
- Checks: `cargo build` ok. `cargo test --release` 46 passed, 1 ignored.
  Debug `cargo test`: 45 passed, 1 failed (the flaky timing test above), 1
  ignored. Guard output empty: yes; `src/main.rs` hunks as listed above,
  `src/settings.rs` unchanged. Clippy new warnings: no. `rustfmt --check` on
  `settings_dialog.rs`: clean. `timeout 5 cargo run`: starts, no panic.
- Deviations from the PRD: none.
- Open items for manual QA: Two-page spreads on and off with a comic open.

## Step 3: Image processing subpage and summary row (`b5a150f`)

- State: done.
- Note: this section was meant to be part of `b5a150f`, but it was staged
  too late and went into the step 4 commit instead. No amend, so the history
  keeps one commit per step.
- Changed: `src/settings_dialog.rs` (old four-switch group replaced; new
  `image_processing_page`; temporary dead-code allow removed),
  `src/settings.rs` (doc comment on `auto_contrast` only: it is now
  independent of the scaling choice in the UI). `src/main.rs` is unchanged
  in this step.
- Main page: an untitled group with the activatable "Image processing"
  row, the summary as subtitle and a `go-next-symbolic` suffix. Activating
  it pushes the subpage.
- Subpage: `AdwNavigationPage` "Image processing" with an `AdwToolbarView`,
  an `AdwHeaderBar` and an `AdwPreferencesPage`. Scaling group: three radio
  rows (one `GtkCheckButton` group, each row's activatable widget is its
  check button) with the titles and subtitles listed under "Decisions in
  force". Tone group: the "Auto contrast" switch. All initial states are set
  before handlers connect. Scaling writes only `image.scaling`, Auto
  contrast writes only `image.auto_contrast`; both then call the existing
  `rerender` and update the summary. No `RefCell` borrow is held across
  `rerender`. The summary row and the dialog are captured weakly to avoid
  reference cycles.
- Focus: `set_focusable(false)` on the check buttons was tried first and
  failed. The buttons never became Tab stops, but Tab stuck on the selected
  "Default" row and Shift+Tab stuck on the row after it. GTK's check button
  focus handler claims focus for the active radio even when it cannot take
  it. `set_can_focus(false)` fixes this, so the code uses that.
- Probe (throwaway copy in `/tmp/pelta-probe`, nothing committed; it opens
  the real dialog through the Settings button, walks focus with the
  window's `move-focus` signal, the Tab key's binding, and presses rows with
  `GtkListBox`'s `activate-cursor-row`, the Space/Enter binding):
  - Main page Tab cycle: Two-page spreads, Match page border colour, Image
    processing, then wraps.
  - Subpage Tab cycle: back button, Default, Lanczos3 (experimental),
    Mitchell-Netravali / Catmull-Rom, Auto contrast, then wraps. Shift+Tab
    is the exact reverse. No check button is ever focused, and
    `grab_focus()` on each check button returns false.
  - Space on the focused Lanczos3 row: scaling Lanczos3, one rerender,
    summary "Lanczos3". Pressing it again: no write, no rerender.
  - Auto contrast on: scaling stays Lanczos3, summary "Lanczos3, Auto
    contrast". Mitchell, then Default: one rerender each, Auto contrast
    stays on, summaries update while the subpage is open.
  - `navigation.pop` (what Escape and Alt+Left trigger): focus returns to
    the "Image processing" row.
  - Opening the dialog, and reopening and closing it without a change,
    does not rerender (`process_gen` unchanged).
  - Started with `PELTA_LANCZOS3=1 PELTA_AUTO_CONTRAST=1`: Lanczos3
    selected, Auto contrast off, summary "Lanczos3".
- Checks: `cargo build` ok. `cargo test` 46 passed, 1 ignored (debug and
  release). Guard output empty: yes; `src/main.rs` hunks unchanged from step
  2; `src/settings.rs` diff is the doc comment only. Clippy new warnings:
  no. `rustfmt --check` on `settings_dialog.rs`: clean. `timeout 5 cargo
  run`: starts, no panic.
- Deviations from the PRD: the Mitchell row title stays "Mitchell-Netravali
  / Catmull-Rom" with its `main` subtitle (decision: scaling fixes are not
  in this base). The summary still says "Mitchell-Netravali", as PRD 5.3
  specifies.
- Open items for manual QA: clicking the title, subtitle and radio of each
  scaling row with the mouse; real Escape and Alt+Left keys; Ctrl+F does
  nothing; narrow width (bottom sheet at 450 px wide or 360 px high);
  filter plus Auto contrast on a large page; visible page change after each
  selection with a comic open.

## Step 4: version 0.1.8

- State: committed; local Flatpak build pending.
- Changed: `Cargo.toml` (version line only), `Cargo.lock` (one line, from
  `cargo update -p pelta-linux-gnome --offline`), `meson.build` (project
  version), `data/com.pelta.ComicReader.metainfo.xml` (new 0.1.8 release
  dated 2026-10-06 at the top of `<releases>`).
- Checks: `appstreamcli validate --no-net --explain`: "Validation was
  successful: pedantic: 1" (the same pedantic note exists on `main`).
  `cargo build` ok. `cargo test` 46 passed, 1 ignored (debug and release).
  Guard output empty: yes; `src/main.rs` and `src/settings.rs` hunks
  unchanged from step 3. Clippy new warnings: no. `rustfmt --check` on
  `settings_dialog.rs`: clean.
- Local Flatpak (`flatpak-builder --user --install`, nothing pushed):
  pending.
- Deviations from the PRD: none.
- Open items for manual QA: the full checklist in PRD section 12 on the
  installed Flatpak, minus screenshot comparison. The local `--user` build
  and the system 0.1.7 install share one settings file (review R7).

## Step 5: Image processing on the main page (uncommitted)

- State: done in the working tree, not committed.
- Request: no subpage. Scaling and Auto contrast are on the same Settings
  page. This replaces the step 3 subpage and the summary row, and the PRD
  sections that describe them.
- Changed: `src/settings_dialog.rs`. `image_processing_page` is now
  `add_image_processing_groups`, which adds the "Scaling" and "Tone" groups
  to the main page under "Appearance". The "Image processing" row, the
  subpage, `scaling_name`, `image_summary` and the summary test are
  removed. Write and rerender behaviour is unchanged.
  `data/com.pelta.ComicReader.metainfo.xml`: the 0.1.8 release note no
  longer says "its own page".
- Checks: `cargo clippy --all-targets` ok, no warnings in
  `settings_dialog.rs` (all warnings are in files this branch does not
  change). `cargo test` 45 passed, 1 ignored. `rustfmt --check` on
  `settings_dialog.rs`: clean. `cargo fmt` was not run on the crate (see
  "Decisions in force"). `appstreamcli validate --no-net`: successful,
  pedantic: 1.
- Local Flatpak: `flatpak-builder --user --install` from this working tree
  succeeded. Installed `com.pelta.ComicReader` 0.1.8 (user), commit
  `9df198ed`. Nothing pushed, tagged or released.
- Open items for manual QA: the Tab order on the single page; the scaling
  rows and Auto contrast with a comic open.

## Step 6: Reading radios, Auto levels, subtitles (uncommitted)

- State: done in the working tree, not committed. Spec: the "Settings
  Restructure & Auto Levels" PRD given in chat on 2026-10-06.
- Where the PRD does not match the repo, and what was done:
  - Reading mode is not stored in two boolean keys. It is one in-memory
    `ReadingMode` enum, so a stored "both on" or "both off" state cannot
    exist. The precedence ("Single page" wins) is in
    `reading_mode_from_radios`, which maps the two radio states to the
    enum. The handler reads both radios at every `toggled`, so it writes
    exactly once in either GTK signal order.
  - Matte from the processed page versus "Match page border colour must not
    change": the matte comes from the processed page (after Auto levels and
    CLAHE) only while Auto levels is on. With Auto levels off it still comes
    from the raw page, so CLAHE-only and scaling-only behaviour is unchanged.
  - There is no crop stage. The order is decode, scale, Auto levels, CLAHE.
- Changed:
  - New `src/auto_levels.rs`: luma histogram on a nearest-sampled proxy
    (long side 512), black and white points at 0.5% and 99.5% (white
    counted from the top), skip rules (empty or transparent, white point
    under 96, spread under 48, already within 4 of 0 and 255), 256-entry
    LUT applied equally to R, G and B (hue kept), alpha unchanged.
  - `src/image_proc.rs`: one new `if settings.auto_levels` call between the
    resize and CLAHE, `auto_levels` in `CacheKey`, doc line. Resamplers and
    CLAHE are unchanged. Test fixture and pipeline tests added.
  - `src/settings.rs`: `ImageProcessingSettings::auto_levels` (also in
    `needs_processing`), key `auto-levels-enabled`, `Preferences` session
    fallback now holds both keys.
  - `data/com.pelta.ComicReader.gschema.xml`: the one new key, default off.
  - `src/reader_matte.rs`: `show` takes an optional `ProcessedSource`
    (slot size and settings). When set, detection runs `process_page` with
    the same inputs as the visible page and reads the result. The cache key
    includes the source. `Preferences` is shared through `Rc`.
  - `src/main.rs`: `mod auto_levels`, `Rc<Preferences>`, Auto levels starts
    from the stored key, `auto_levels` in both cache keys, slot size computed
    before the matte call, `matte_source` passed to `matte.show`.
  - `src/settings_dialog.rs`: Reading radios ("Single page" first), shared
    `add_radio_rows` for Reading and Scaling, "Auto levels" switch below
    Auto contrast (writes the setting and the key, then the existing
    `rerender`), no trailing full stops in subtitles.
  - Release note for 0.1.8 and README feature list.
- Checks: `cargo test` debug 69 passed, 1 failed (the flaky timing test
  above; passes alone), 1 ignored; `cargo test --release` 70 passed, 1
  ignored. Byte identity: output hashes of all six (scaling, Auto contrast)
  pairs were recorded before any change and the test reproduces them with
  Auto levels off (filter hashes only on x86_64, because
  `fast_image_resize` picks SIMD code per CPU). Clippy: 9 warnings, 11 with
  tests, the same as before; the new `open_settings_dialog` parameter is
  covered by `#[allow(clippy::too_many_arguments)]`. `rustfmt --check`
  clean on `auto_levels.rs`, `settings_dialog.rs`, `reader_matte.rs`,
  `settings.rs`; the remaining rustfmt differences in `image_proc.rs` and
  `main.rs` are older lines. `cargo fmt` was not run on the crate.
- Probe on the QA comic (The Killer 001, 33 pages, digital): white point
  is 255 on every page, black point 0 to 40. Auto levels deepened blacks on
  25 pages and skipped 8 that already use the full range; 2 to 7 ms per
  page at 900×1300. The matte stays white. This book has no grey paper, so
  the "grey scan" QA item needs a scanned comic.
- Local Flatpak: `flatpak-builder --user --install` succeeded. Installed
  `com.pelta.ComicReader` 0.1.8 (user), commit `69e51be1`; the installed
  schema lists `auto-levels-enabled` and `page-matte-detection`. Nothing
  pushed, tagged or released.
- Open items for manual QA: the checklist in the PRD (section 6).
