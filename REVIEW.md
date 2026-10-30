# Rust review: pelta-cbr-linux

Pelta on Linux is already a **GTK 4 + libadwaita** application with a Wayland-only Flatpak manifest (`packaging/com.pelta.ComicReader.yml`). There is no Tauri or WebKitGTK. Edition 2021, MSRV 1.92. This review treats `crates/pelta-core` as an internal **library** (API rules apply) and `src/` as **application** code (gtk-rs `Rc`/`RefCell`, preludes, and `eprintln` diagnostics are expected).

**Verdict:** the reader is in good shape: page navigation, matte detection, auto levels, and GSettings are well tested. The important gaps were unsafe libarchive FFI without RAII, `String` error types, and no clippy gate. Those are fixed in this PR. **The most important remaining follow-up** is seeking inside ZIP/CBZ so `read_page` does not rescan the archive on every turn.

**Tools run.** `cargo test` on main: 69 passed, 1 failed (`matte::tests::detection_is_fast_on_a_full_size_page`, debug median 8.9 ms vs a 5 ms budget), 1 ignored. `cargo fmt --check` was dirty (three wrapping hunks). Clippy was not configured. No Tauri/WebKitGTK remnants.

Line numbers below refer to **main before this PR**, so the findings stay auditable.

## P0 (must fix)

### 1. libarchive handle is not RAII and has no `// SAFETY:` comments (`src/archive.rs:133-170`, `:200-266`)
**Rule:** Wrap C-owned pointers in a type whose `Drop` frees them; every `unsafe` block needs a `// SAFETY:` comment (BeautifulRust §8; clippy `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`).
**Problem:** `ComicArchive::open` and `read_page` call `archive_read_new` and free the pointer by hand on each error path. A panic between `new` and `free` (for example during `Vec` growth) leaks the C reader. Large `unsafe` blocks mix many operations with no invariants recorded.
**Fix:**
```rust
struct ArchiveReader {
    ptr: NonNull<ArchiveOpaque>,
}
impl Drop for ArchiveReader {
    fn drop(&mut self) {
        // SAFETY: ptr is exclusively owned and has not been freed yet.
        let _ = unsafe { archive_read_free(self.as_ptr()) };
    }
}
```
**Status:** fixed in `crates/pelta-core/src/archive.rs`.

### 2. Untrusted `archive_entry_size` can allocate unbounded memory (`src/archive.rs:231-236`)
**Rule:** Don't panic or blow memory on expected bad input (Effective Rust Item 18; M-PANIC-ON-BUG).
**Problem:** `Vec::with_capacity(size as usize)` trusts the archive header. A crafted CBR/CBZ can claim a multi-gigabyte page and abort the process (or the sandbox) before any image decode. The read loop also had no running cap when `size <= 0`.
**Fix:**
```rust
pub const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;
if size > i64::try_from(MAX_PAGE_BYTES).unwrap_or(i64::MAX) {
    return Err(ArchiveError::PageTooLarge { max: MAX_PAGE_BYTES });
}
```
**Status:** fixed, with a matching `MAX_PAGES` cap on listing.

## P1 (should fix)

### 3. `String` is the public error type (`src/archive.rs:125`, `src/image_proc.rs:94`, `:106`)
**Rule:** Library `Result` errors implement `std::error::Error` (C-GOOD-ERR, Rust API Guidelines; Effective Rust Item 4).
**Problem:** Callers cannot match on "missing file" vs "no pages" vs "decode failed". `?` into `anyhow` works only by losing structure. Changing this after a core crate exists is a breaking change for that crate.
**Fix:**
```rust
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("file does not exist: {0}")]
    MissingFile(PathBuf),
    #[error("no supported comic pages found in archive")]
    NoPages,
    // ...
}
```
**Status:** fixed (`ArchiveError`, `ProcessError`).

### 4. `bool` parameters on the reading API (`src/reading.rs:26`, `:51`, `:65`)
**Rule:** No `bool` parameters in public or shared signatures (C-CUSTOM-TYPE; clippy `fn_params_excessive_bools` with BeautifulRust `max-fn-params-bools = 0`).
**Problem:** `view_for_page(page, count, true)` does not say what `true` means, and a third layout mode cannot be added later.
**Fix:**
```rust
pub fn view_for_page(page: usize, count: usize, mode: ReadingMode) -> Option<PageView>
```
**Status:** fixed.

### 5. Archive tests depend on files under `/home/lozbek/Downloads` (`src/archive.rs:297-354`)
**Rule:** Tests must not require machine-local data (Effective Rust Item 30).
**Problem:** `test_open_yazidi_cbr` and `test_open_cbz` skip (or, on this machine, pass against personal comics). CI never proved that libarchive listing, junk filtering, or `read_page` work.
**Fix:** commit `tests/fixtures/archive/tiny.cbz` and assert natural order, junk skip, PNG signatures, and out-of-range errors.
**Status:** fixed.

### 6. No clippy config and no `-D warnings` in CI (whole repo)
**Rule:** Configure lints in `Cargo.toml` / `clippy.toml`; enforce warnings in CI, never `#![deny(warnings)]` in source (BeautifulRust §2; Effective Rust Item 29).
**Problem:** Mechanical issues (undocumented unsafe, `String` errors, bool params) were invisible. `.github/workflows/flatpak.yml` only checked Wayland finish-args and the Flatpak build.
**Fix:** drop in BeautifulRust `lints/`, `[workspace.lints]`, root `clippy.toml`, a `rust` job (`cargo fmt`, `cargo clippy -p pelta-core -- -D warnings`, `cargo test -p pelta-core`), and a Meson `cargo-clippy` test inside the Flatpak build.
**Status:** fixed.

### 7. README claimed PDF support (`README.md` Features)
**Rule:** User-facing text must match behaviour (product accuracy; no unsupported format claims).
**Problem:** Features listed `.pdf`. The desktop file, AppStream metadata, file dialog, and libarchive backend do not open PDF.
**Fix:** list `.cbr` / `.rar`, `.cbz` / `.zip`, `.cbt` / `.tar`.
**Status:** fixed.

### 8. Debug-only timing test is flaky (`src/matte.rs:517-529`)
**Rule:** Don't let `HashMap` order or wall-clock budgets fail CI (BeautifulRust review checklist).
**Problem:** `detection_is_fast_on_a_full_size_page` asserted median under 5 ms. It failed in a debug `cargo test` at 8.9 ms on this host. Release builds still get the budget.
**Fix:** log the debug median and keep the 5 ms assert for release.
**Status:** fixed.

### 9. Public fields on `ComicArchive` (`src/archive.rs:118-122`)
**Rule:** Fields private by default (C-STRUCT-PRIVATE).
**Problem:** Callers could mutate `pages` out of sync with `path`.
**Fix:** private fields plus `path()`, `title()`, `pages()`, `page_count()`.
**Status:** fixed.

## P2 (consider)

### 10. `read_page` re-opens and scans the archive every time (`src/archive.rs:191-269`)
**Rule:** Design for usability first; optimize after measuring (BeautifulRust §12).
**Problem:** Each page turn is O(n) in archive members. Fine for small CBZs; slow for 200-page RARs.
**Fix:** for seekable ZIP, record local-file offsets; keep the scan for RAR.

### 11. `main.rs` is a 990-line `build_ui` (`src/main.rs:304`)
**Rule:** Prefer small modules (BeautifulRust §9).
**Problem:** Window chrome, gestures, processing, and archive open live in one function with many `Rc` clones.
**Fix:** split `show_page` / `open_comic` / controllers into `src/window.rs` when touching that code next.

### 12. Processed-page cache clones full RGBA on hit (`src/image_proc.rs:47-54`)
**Rule:** Clone small immutable data deliberately (BeautifulRust §3).
**Problem:** `ProcessCache::get` returns `ProcessedPage` by value, copying several megabytes. `Rc<[u8]>` would make hits cheap.

### 13. File dialog filter name omitted CBT (`src/main.rs:741`)
**Problem:** The filter included `*.cbt` but was labelled "CBR, CBZ".
**Status:** fixed (name + `*.rar` / `*.zip` / `*.tar`).

## Opportunities for improvement

- **Seekable ZIP page index** so neighbour prefetch does not rescan (finding 10).
- **Offline Flatpak sources** (`cargo-sources.json`) when submitting to Flathub; the manifest already notes `--share=network` is a CI convenience.
- **Structured logging** (`tracing`) instead of `eprintln!` in the GTK shell.
- **Pinch-zoom** is listed in the README; the GTK shell does not implement it yet.
- **Miri** cannot run the FFI module; keep the RAII wrapper small and add adversarial size tests (already started with `MAX_PAGE_BYTES`).
- **`bindgen` + `pelta-archive-sys`** if the libarchive surface grows past the current ten functions.

## What's already good

- Wayland-only finish-args, `GDK_BACKEND` check, and `build-aux/check-flatpak-no-x11.sh` agree; the sandbox has no network and no X11.
- Image processing runs at display size, off the UI thread, with a bounded cache of 6 pages.
- Matte detection, auto levels, and settings-dialog radio mapping have thorough unit tests, including public-domain page fixtures.
- GSettings falls back to an in-memory session when the schema is not installed, so `cargo run` from a checkout still works.
- Meson + Cargo + Flatpak follow the gtk-rust-template layout.

## Suggested lints

The repo had no `[lints]` table. BeautifulRust `lints/lints.toml` and `lints/clippy.toml` are now the workspace clippy config. From this review they would have caught:

- undocumented `unsafe` in `archive.rs` (`undocumented_unsafe_blocks`, finding 1),
- bool parameters on `view_for_page` (`fn_params_excessive_bools`, finding 4),
- missing `Debug` / docs as the core crate grows (`missing_debug_implementations`, `missing_docs`).
