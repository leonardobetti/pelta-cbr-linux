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
