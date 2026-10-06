//! Settings dialog and the pure mapping between its controls and
//! [`AppSettings`](crate::settings::AppSettings).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use crate::image_proc::ProcessCache;
use crate::reader_matte::ReaderMatte;
use crate::settings::{AppSettings, ImageProcessingSettings, ReadingMode, ScalingMode};
use crate::AppState;

/// Preferences dialog (not ShortcutsDialog — that widget is for keyboard
/// shortcut lists and cannot host SwitchRows / preference groups).
pub fn open_settings_dialog(
    window: &adw::ApplicationWindow,
    settings: &Rc<RefCell<AppSettings>>,
    show_page: &Rc<impl Fn(usize) + 'static>,
    state: &Rc<RefCell<AppState>>,
    process_cache: &Rc<RefCell<ProcessCache>>,
    process_gen: &Rc<Cell<u64>>,
    matte: &Rc<ReaderMatte>,
) {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Settings");
    dialog.set_search_enabled(false);

    let page = adw::PreferencesPage::new();
    page.set_title("General");

    let reading_group = adw::PreferencesGroup::new();
    reading_group.set_title("Reading");

    let two_page_row = adw::SwitchRow::new();
    two_page_row.set_title("Two-page spreads");
    two_page_row.set_subtitle("Facing pages; cover and back cover stay alone.");
    two_page_row.set_active(switch_from_reading_mode(settings.borrow().reading.mode));

    let rerender = {
        let show_page = show_page.clone();
        let state = state.clone();
        let process_cache = process_cache.clone();
        let process_gen = process_gen.clone();
        move || {
            process_cache.borrow_mut().clear();
            process_gen.set(process_gen.get().wrapping_add(1));
            let page = state.borrow().current_page;
            if state.borrow().archive.is_some() {
                show_page(page);
            }
        }
    };

    two_page_row.connect_active_notify({
        let settings = settings.clone();
        let show_page = show_page.clone();
        let state = state.clone();
        move |row| {
            let current = settings.borrow().reading.mode;
            let Some(mode) = reading_write(row.is_active(), current) else {
                return;
            };
            settings.borrow_mut().reading.mode = mode;
            let page = state.borrow().current_page;
            if state.borrow().archive.is_some() {
                show_page(page);
            }
        }
    });

    reading_group.add(&two_page_row);
    page.add(&reading_group);

    let appearance_group = adw::PreferencesGroup::new();
    appearance_group.set_title("Appearance");
    let matte_row = adw::SwitchRow::new();
    matte_row.set_title("Match page border colour");
    matte_row.set_subtitle("Use each page's border colour as the reader background.");
    matte_row.set_active(matte.enabled());
    matte_row.connect_active_notify({
        let matte = matte.clone();
        let show_page = show_page.clone();
        let state = state.clone();
        move |row| {
            let enabled = row.is_active();
            if enabled == matte.enabled() {
                return;
            }
            matte.set_enabled(enabled);
            let page = state.borrow().current_page;
            if enabled && state.borrow().archive.is_some() {
                show_page(page);
            }
        }
    });
    appearance_group.add(&matte_row);
    page.add(&appearance_group);

    let image_row = adw::ActionRow::new();
    image_row.set_title("Image processing");
    image_row.set_subtitle(&image_summary(settings.borrow().image));
    image_row.set_activatable(true);
    image_row.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
    let image_page = image_processing_page(settings, rerender, &image_row);
    image_row.connect_activated({
        let dialog = dialog.downgrade();
        move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.push_subpage(&image_page);
            }
        }
    });
    let image_group = adw::PreferencesGroup::new();
    image_group.add(&image_row);
    page.add(&image_group);

    dialog.add(&page);
    dialog.present(Some(window));
}

/// The "Image processing" subpage. Its handlers update `summary_row`, which
/// is held weakly because the row's own handler keeps this page alive.
fn image_processing_page(
    settings: &Rc<RefCell<AppSettings>>,
    rerender: impl Fn() + Clone + 'static,
    summary_row: &adw::ActionRow,
) -> adw::NavigationPage {
    let content = adw::PreferencesPage::new();

    let scaling_group = adw::PreferencesGroup::new();
    scaling_group.set_title("Scaling");
    let current = settings.borrow().image.scaling;
    let mut checks: Vec<(ScalingMode, gtk4::CheckButton)> = Vec::new();
    for (mode, title, subtitle) in [
        (ScalingMode::Nothing, "Default", "GTK scales the page."),
        (
            ScalingMode::Lanczos3,
            "Lanczos3 (experimental)",
            "Linear-light Lanczos3 resize to display size",
        ),
        (
            ScalingMode::MitchellNetravali,
            "Mitchell-Netravali / Catmull-Rom",
            "Linear-light Mitchell resize to display size",
        ),
    ] {
        let check = gtk4::CheckButton::new();
        check.set_valign(gtk4::Align::Center);
        // Keyboard focus belongs to the row; Space or Enter on it selects.
        // `set_focusable(false)` is not enough: the active radio's focus
        // handler still claims Tab and traps focus on its row.
        check.set_can_focus(false);
        check.set_group(checks.first().map(|(_, first)| first));
        check.set_active(mode == current);

        let row = adw::ActionRow::new();
        row.set_title(title);
        row.set_subtitle(subtitle);
        row.add_prefix(&check);
        row.set_activatable_widget(Some(&check));
        scaling_group.add(&row);
        checks.push((mode, check));
    }
    content.add(&scaling_group);

    let tone_group = adw::PreferencesGroup::new();
    tone_group.set_title("Tone");
    let contrast_row = adw::SwitchRow::new();
    contrast_row.set_title("Auto contrast");
    contrast_row.set_subtitle("CLAHE on luminance (tile-based, clip-limited)");
    contrast_row.set_active(settings.borrow().image.auto_contrast);
    tone_group.add(&contrast_row);
    content.add(&tone_group);

    let update_summary = {
        let settings = settings.clone();
        let summary_row = summary_row.downgrade();
        move || {
            if let Some(row) = summary_row.upgrade() {
                row.set_subtitle(&image_summary(settings.borrow().image));
            }
        }
    };

    for (mode, check) in &checks {
        let mode = *mode;
        check.connect_toggled({
            let settings = settings.clone();
            let rerender = rerender.clone();
            let update_summary = update_summary.clone();
            move |check| {
                let current = settings.borrow().image.scaling;
                let Some(scaling) = scaling_write(mode, check.is_active(), current) else {
                    return;
                };
                settings.borrow_mut().image.scaling = scaling;
                rerender();
                update_summary();
            }
        });
    }

    contrast_row.connect_active_notify({
        let settings = settings.clone();
        move |row| {
            let current = settings.borrow().image.auto_contrast;
            let Some(on) = contrast_write(row.is_active(), current) else {
                return;
            };
            settings.borrow_mut().image.auto_contrast = on;
            rerender();
            update_summary();
        }
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&content));
    adw::NavigationPage::new(&toolbar, "Image processing")
}

fn reading_mode_from_switch(on: bool) -> ReadingMode {
    if on {
        ReadingMode::TwoPageSpreads
    } else {
        ReadingMode::SinglePage
    }
}

fn switch_from_reading_mode(mode: ReadingMode) -> bool {
    mode.is_two_page()
}

/// The reading mode to store when the "Two-page spreads" switch reports
/// `switch_on`, or `None` when it matches `current`.
fn reading_write(switch_on: bool, current: ReadingMode) -> Option<ReadingMode> {
    let mode = reading_mode_from_switch(switch_on);
    (mode != current).then_some(mode)
}

/// The scaling mode to store when the radio button for `row` emits `toggled`.
///
/// Selecting a row emits `toggled` on the previous button (inactive) and then
/// on the new one (active); only the second one writes, and only once.
fn scaling_write(row: ScalingMode, now_active: bool, current: ScalingMode) -> Option<ScalingMode> {
    (now_active && row != current).then_some(row)
}

fn contrast_write(on: bool, current: bool) -> Option<bool> {
    (on != current).then_some(on)
}

fn scaling_name(mode: ScalingMode) -> &'static str {
    match mode {
        ScalingMode::Nothing => "Default",
        ScalingMode::Lanczos3 => "Lanczos3",
        ScalingMode::MitchellNetravali => "Mitchell-Netravali",
    }
}

/// Subtitle of the "Image processing" row on the main page.
fn image_summary(image: ImageProcessingSettings) -> String {
    let name = scaling_name(image.scaling);
    if image.auto_contrast {
        format!("{name}, Auto contrast")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALING: [ScalingMode; 3] = [
        ScalingMode::Nothing,
        ScalingMode::Lanczos3,
        ScalingMode::MitchellNetravali,
    ];

    fn image(scaling: ScalingMode, auto_contrast: bool) -> ImageProcessingSettings {
        ImageProcessingSettings {
            scaling,
            auto_contrast,
        }
    }

    /// Feeds `toggled` signals to the scaling handler logic, as GTK delivers
    /// them, and returns every value it would write.
    fn scaling_writes(start: ScalingMode, signals: &[(ScalingMode, bool)]) -> Vec<ScalingMode> {
        let mut current = start;
        let mut writes = Vec::new();
        for &(row, active) in signals {
            if let Some(mode) = scaling_write(row, active, current) {
                current = mode;
                writes.push(mode);
            }
        }
        writes
    }

    /// The new controls: pick a scaling row, then set the Auto contrast switch.
    fn new_dialog(
        start: ImageProcessingSettings,
        row: ScalingMode,
        contrast_on: bool,
    ) -> (ScalingMode, bool) {
        let mut s = start;
        if let Some(mode) = scaling_write(s.scaling, false, s.scaling) {
            s.scaling = mode;
        }
        if let Some(mode) = scaling_write(row, true, s.scaling) {
            s.scaling = mode;
        }
        if let Some(on) = contrast_write(contrast_on, s.auto_contrast) {
            s.auto_contrast = on;
        }
        (s.scaling, s.auto_contrast)
    }

    #[test]
    fn summary_covers_all_six_combinations() {
        let cases = [
            (ScalingMode::Nothing, false, "Default"),
            (ScalingMode::Nothing, true, "Default, Auto contrast"),
            (ScalingMode::Lanczos3, false, "Lanczos3"),
            (ScalingMode::Lanczos3, true, "Lanczos3, Auto contrast"),
            (ScalingMode::MitchellNetravali, false, "Mitchell-Netravali"),
            (
                ScalingMode::MitchellNetravali,
                true,
                "Mitchell-Netravali, Auto contrast",
            ),
        ];
        for (scaling, contrast, expected) in cases {
            assert_eq!(image_summary(image(scaling, contrast)), expected);
        }
    }

    #[test]
    fn reading_switch_maps_both_ways() {
        assert_eq!(reading_mode_from_switch(false), ReadingMode::SinglePage);
        assert_eq!(reading_mode_from_switch(true), ReadingMode::TwoPageSpreads);
        assert!(!switch_from_reading_mode(ReadingMode::SinglePage));
        assert!(switch_from_reading_mode(ReadingMode::TwoPageSpreads));
        for mode in [ReadingMode::SinglePage, ReadingMode::TwoPageSpreads] {
            assert_eq!(
                reading_mode_from_switch(switch_from_reading_mode(mode)),
                mode
            );
        }
    }

    #[test]
    fn selecting_a_new_row_writes_once() {
        let signals = [(ScalingMode::Nothing, false), (ScalingMode::Lanczos3, true)];
        assert_eq!(
            scaling_writes(ScalingMode::Nothing, &signals),
            vec![ScalingMode::Lanczos3]
        );
    }

    #[test]
    fn reactivating_the_selected_row_writes_nothing() {
        for mode in SCALING {
            assert!(scaling_writes(mode, &[]).is_empty());
        }
    }

    #[test]
    fn initial_state_writes_nothing() {
        for current in SCALING {
            for row in SCALING {
                assert_eq!(scaling_write(row, row == current, current), None);
            }
        }
        for mode in [ReadingMode::SinglePage, ReadingMode::TwoPageSpreads] {
            assert_eq!(reading_write(switch_from_reading_mode(mode), mode), None);
        }
        for on in [false, true] {
            assert_eq!(contrast_write(on, on), None);
        }
    }

    #[test]
    fn switches_write_only_on_change() {
        assert_eq!(
            reading_write(true, ReadingMode::SinglePage),
            Some(ReadingMode::TwoPageSpreads)
        );
        assert_eq!(
            reading_write(false, ReadingMode::TwoPageSpreads),
            Some(ReadingMode::SinglePage)
        );
        assert_eq!(contrast_write(true, false), Some(true));
        assert_eq!(contrast_write(false, true), Some(false));
    }

    #[test]
    fn new_controls_write_what_the_old_dialog_wrote() {
        // (scaling, auto_contrast) written by the four exclusive switches in 0.1.7.
        let old_nothing = (ScalingMode::Nothing, false);
        let old_lanczos = (ScalingMode::Lanczos3, false);
        let old_mitchell = (ScalingMode::MitchellNetravali, false);
        let old_contrast = (ScalingMode::Nothing, true);

        for start_scaling in SCALING {
            for start_contrast in [false, true] {
                let start = image(start_scaling, start_contrast);
                assert_eq!(new_dialog(start, ScalingMode::Nothing, false), old_nothing);
                assert_eq!(new_dialog(start, ScalingMode::Lanczos3, false), old_lanczos);
                assert_eq!(
                    new_dialog(start, ScalingMode::MitchellNetravali, false),
                    old_mitchell
                );
                assert_eq!(new_dialog(start, ScalingMode::Nothing, true), old_contrast);
            }
        }
    }

    #[test]
    fn scaling_and_contrast_stay_independent() {
        for row in SCALING {
            assert_eq!(
                new_dialog(image(ScalingMode::Nothing, true), row, true),
                (row, true)
            );
        }
    }

    #[test]
    fn reading_switch_writes_what_the_old_switches_wrote() {
        // Old "Single-page" on wrote SinglePage; old "Two-page spreads" on wrote TwoPageSpreads.
        for current in [ReadingMode::SinglePage, ReadingMode::TwoPageSpreads] {
            let off = reading_write(false, current).unwrap_or(current);
            let on = reading_write(true, current).unwrap_or(current);
            assert_eq!(off, ReadingMode::SinglePage);
            assert_eq!(on, ReadingMode::TwoPageSpreads);
        }
    }
}
