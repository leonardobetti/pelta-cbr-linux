//! Applies the detected page matte to the reader background.
//!
//! Detection runs off the UI thread. Results are cached for the open book and
//! only applied if they still belong to the view on screen, so fast page
//! turns never show an older page's colour. The colour change is a CSS
//! transition, which GTK skips when `gtk-enable-animations` is off.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::image_proc::{decode_rgba, process_page};
use crate::matte::{detect_page, detect_spread, Rgb};
use crate::reading::PageView;
use crate::settings::{ImageProcessingSettings, Preferences};

const READER_CLASS: &str = "pelta-reader";
const TRANSITION: &str = "transition: background-color 200ms ease-out, color 200ms ease-out;";

/// Detect from the page as the pipeline renders it at `width`×`height` with
/// `settings`, instead of from the raw decoded page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessedSource {
    pub width: u32,
    pub height: u32,
    pub settings: ImageProcessingSettings,
}

type MatteKey = (PageView, Option<ProcessedSource>);

pub struct ReaderMatte {
    prefs: Rc<Preferences>,
    provider: gtk4::CssProvider,
    cache: RefCell<HashMap<MatteKey, Option<Rgb>>>,
    /// Bumped when the book changes or the feature is switched off, so work
    /// started before then is dropped.
    generation: Cell<u64>,
    on_screen: Cell<Option<MatteKey>>,
    applied: Cell<Option<Rgb>>,
}

impl ReaderMatte {
    pub fn new(reader: &impl IsA<gtk4::Widget>, prefs: Rc<Preferences>) -> Rc<Self> {
        reader.add_css_class(READER_CLASS);
        let provider = gtk4::CssProvider::new();
        gtk4::style_context_add_provider_for_display(
            &reader.display(),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let this = Rc::new(Self {
            prefs,
            provider,
            cache: RefCell::default(),
            generation: Cell::new(0),
            on_screen: Cell::new(None),
            applied: Cell::new(None),
        });
        this.load_css(None);
        this
    }

    pub fn enabled(&self) -> bool {
        self.prefs.page_matte()
    }

    /// Persists the setting. Switching off returns to the theme background at
    /// once and drops any pending detection; switching on takes effect on the
    /// next [`Self::show`].
    pub fn set_enabled(&self, enabled: bool) {
        self.prefs.set_page_matte(enabled);
        if !enabled {
            self.bump();
            self.cache.borrow_mut().clear();
            self.apply(None);
        }
    }

    /// A different book was opened.
    pub fn reset(&self) {
        self.bump();
        self.cache.borrow_mut().clear();
    }

    /// `view` is now on screen; `pages` holds its encoded page bytes (one for
    /// a single page, left then right for a spread). With `processed`, the
    /// colour comes from the pages as rendered with those settings; with
    /// `None`, from the raw pages. Does nothing while the feature is off.
    pub fn show(
        self: &Rc<Self>,
        view: PageView,
        pages: Vec<Vec<u8>>,
        processed: Option<ProcessedSource>,
    ) {
        if !self.enabled() {
            return;
        }
        let key = (view, processed);
        self.on_screen.set(Some(key));
        if let Some(&cached) = self.cache.borrow().get(&key) {
            self.apply(cached);
            return;
        }
        let generation = self.generation.get();
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || detect(&pages, processed)).await;
            let Some(this) = this.upgrade() else {
                return;
            };
            if this.generation.get() != generation {
                return;
            }
            let matte = match result {
                Ok(Ok(matte)) => matte,
                Ok(Err(e)) => {
                    eprintln!("[pelta-linux-gnome] page matte detection failed: {e}");
                    None
                }
                Err(_) => None,
            };
            this.cache.borrow_mut().insert(key, matte);
            if this.on_screen.get() == Some(key) {
                this.apply(matte);
            }
        });
    }

    fn bump(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.on_screen.set(None);
    }

    fn apply(&self, matte: Option<Rgb>) {
        if self.applied.get() != matte {
            self.applied.set(matte);
            self.load_css(matte);
        }
    }

    fn load_css(&self, matte: Option<Rgb>) {
        self.provider.load_from_string(&reader_css(matte));
    }
}

fn detect(pages: &[Vec<u8>], processed: Option<ProcessedSource>) -> Result<Option<Rgb>, String> {
    let load = |bytes: &[u8]| match processed {
        None => decode_rgba(bytes),
        Some(p) => {
            let page = process_page(bytes, p.width, p.height, p.settings)?;
            image::RgbaImage::from_raw(page.width, page.height, page.rgba)
                .ok_or_else(|| "processed page has the wrong size".to_string())
        }
    };
    match pages {
        [page] => Ok(detect_page(&load(page)?)),
        [left, right] => Ok(detect_spread(&load(left)?, &load(right)?)),
        _ => Err(format!("expected 1 or 2 pages, got {}", pages.len())),
    }
}

/// Stylesheet for the reader: the matte colour, or the window background when
/// there is none, with a foreground that stays legible on it.
fn reader_css(matte: Option<Rgb>) -> String {
    match matte {
        None => {
            format!(".{READER_CLASS} {{ background-color: var(--window-bg-color); {TRANSITION} }}")
        }
        Some(m) => {
            let [r, g, b] = m.0;
            let fg = if m.prefers_dark_foreground() {
                "rgba(0, 0, 0, 0.8)"
            } else {
                "rgba(255, 255, 255, 0.9)"
            };
            format!(
                ".{READER_CLASS} {{ background-color: rgb({r}, {g}, {b}); color: {fg}; {TRANSITION} }}"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_proc::fixtures;

    #[test]
    fn css_without_matte_uses_the_theme_background() {
        let css = reader_css(None);
        assert!(css.contains("var(--window-bg-color)"));
        assert!(!css.contains("color: rgba"));
        assert!(css.contains("transition: background-color 200ms"));
    }

    #[test]
    fn css_foreground_contrasts_with_the_matte() {
        let light = reader_css(Some(Rgb([243, 232, 205])));
        assert!(light.contains("background-color: rgb(243, 232, 205)"));
        assert!(light.contains("color: rgba(0, 0, 0"));
        let dark = reader_css(Some(Rgb([10, 10, 20])));
        assert!(dark.contains("color: rgba(255, 255, 255"));
        assert!(dark.contains("transition: background-color 200ms"));
    }

    #[test]
    fn detect_rejects_wrong_page_counts() {
        assert!(detect(&[], None).is_err());
        assert!(detect(&[vec![], vec![], vec![]], None).is_err());
        assert!(detect(&[b"not an image".to_vec()], None).is_err());
    }

    fn levels_source(auto_contrast: bool) -> ProcessedSource {
        ProcessedSource {
            width: 180,
            height: 240,
            settings: ImageProcessingSettings {
                auto_contrast,
                auto_levels: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn matte_comes_from_the_processed_page() {
        let scan = fixtures::grey_scan();
        let bytes = fixtures::png(&scan);
        let pages = std::slice::from_ref(&bytes);

        let raw = detect(pages, None).unwrap().expect("raw matte");
        assert_eq!(raw, detect_page(&scan).unwrap());

        for contrast in [false, true] {
            let source = levels_source(contrast);
            let shown = process_page(&bytes, source.width, source.height, source.settings).unwrap();
            let shown = image::RgbaImage::from_raw(shown.width, shown.height, shown.rgba).unwrap();
            let processed = detect(pages, Some(source)).unwrap();
            assert_eq!(processed, detect_page(&shown), "auto contrast {contrast}");
        }

        let brightened = detect(pages, Some(levels_source(false)))
            .unwrap()
            .expect("processed matte");
        assert!(
            brightened.relative_luminance() > raw.relative_luminance() + 0.1,
            "raw {raw:?}, processed {brightened:?}"
        );
    }

    #[test]
    fn processed_spread_uses_both_processed_pages() {
        let bytes = fixtures::png(&fixtures::grey_scan());
        let pages = [bytes.clone(), bytes];
        let raw = detect(&pages, None).unwrap().unwrap();
        let processed = detect(&pages, Some(levels_source(false))).unwrap().unwrap();
        assert_ne!(raw, processed);
    }
}
