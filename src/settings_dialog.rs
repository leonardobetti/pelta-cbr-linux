//! Settings dialog and the pure mapping between its controls and
//! [`AppSettings`](crate::settings::AppSettings).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use crate::image_proc::ProcessCache;
use crate::reader_matte::ReaderMatte;
use crate::settings::{AppSettings, Preferences, ReadingMode, ScalingMode};
use crate::AppState;

/// Preferences dialog (not ShortcutsDialog — that widget is for keyboard
/// shortcut lists and cannot host SwitchRows / preference groups).
#[allow(clippy::too_many_arguments)]
pub fn open_settings_dialog(
    window: &adw::ApplicationWindow,
    settings: &Rc<RefCell<AppSettings>>,
    show_page: &Rc<impl Fn(usize) + 'static>,
    state: &Rc<RefCell<AppState>>,
    process_cache: &Rc<RefCell<ProcessCache>>,
    process_gen: &Rc<Cell<u64>>,
    matte: &Rc<ReaderMatte>,
    prefs: &Rc<Preferences>,
) {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Settings");
    dialog.set_search_enabled(false);

    let page = adw::PreferencesPage::new();
    page.set_title("General");

    let reading_group = adw::PreferencesGroup::new();
    reading_group.set_title("Reading");
    let reading_checks = add_radio_rows(
        &reading_group,
        [
            (ReadingMode::SinglePage, "Single page", "One page at a time"),
            (
                ReadingMode::TwoPageSpreads,
                "Two-page spreads",
                "Facing pages; cover and back cover stay alone",
            ),
        ],
        settings.borrow().reading.mode,
    );
    let [single_check, two_page_check] = [&reading_checks[0].1, &reading_checks[1].1];
    for check in [single_check, two_page_check] {
        check.connect_toggled({
            let single = single_check.downgrade();
            let two_page = two_page_check.downgrade();
            let settings = settings.clone();
            let show_page = show_page.clone();
            let state = state.clone();
            move |_| {
                let (Some(single), Some(two_page)) = (single.upgrade(), two_page.upgrade()) else {
                    return;
                };
                let current = settings.borrow().reading.mode;
                let Some(mode) = reading_write(single.is_active(), two_page.is_active(), current)
                else {
                    return;
                };
                settings.borrow_mut().reading.mode = mode;
                let page = state.borrow().current_page;
                if state.borrow().archive.is_some() {
                    show_page(page);
                }
            }
        });
    }
    page.add(&reading_group);

    let appearance_group = adw::PreferencesGroup::new();
    appearance_group.set_title("Appearance");
    let matte_row = adw::SwitchRow::new();
    matte_row.set_title("Match page border colour");
    matte_row.set_subtitle("Use each page's border colour as the reader background");
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
    add_image_processing_groups(&page, settings, prefs, rerender);

    dialog.add(&page);
    dialog.present(Some(window));
}

/// One radio row per item, in order, with the row for `current` selected.
/// Returns each item's check button.
fn add_radio_rows<T: Copy + PartialEq, const N: usize>(
    group: &adw::PreferencesGroup,
    items: [(T, &str, &str); N],
    current: T,
) -> Vec<(T, gtk4::CheckButton)> {
    let mut checks: Vec<(T, gtk4::CheckButton)> = Vec::new();
    for (value, title, subtitle) in items {
        let check = gtk4::CheckButton::new();
        check.set_valign(gtk4::Align::Center);
        // Keyboard focus belongs to the row; Space or Enter on it selects.
        // `set_focusable(false)` is not enough: the active radio's focus
        // handler still claims Tab and traps focus on its row.
        check.set_can_focus(false);
        check.set_group(checks.first().map(|(_, first)| first));
        check.set_active(value == current);

        let row = adw::ActionRow::new();
        row.set_title(title);
        row.set_subtitle(subtitle);
        row.add_prefix(&check);
        row.set_activatable_widget(Some(&check));
        group.add(&row);
        checks.push((value, check));
    }
    checks
}

/// The "Scaling" and "Tone" groups, added to the main page.
fn add_image_processing_groups(
    page: &adw::PreferencesPage,
    settings: &Rc<RefCell<AppSettings>>,
    prefs: &Rc<Preferences>,
    rerender: impl Fn() + Clone + 'static,
) {
    let scaling_group = adw::PreferencesGroup::new();
    scaling_group.set_title("Scaling");
    let checks = add_radio_rows(
        &scaling_group,
        [
            (ScalingMode::Nothing, "Default", "GTK scales the page"),
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
        ],
        settings.borrow().image.scaling,
    );
    page.add(&scaling_group);

    let tone_group = adw::PreferencesGroup::new();
    tone_group.set_title("Tone");
    let contrast_row = adw::SwitchRow::new();
    contrast_row.set_title("Auto contrast");
    contrast_row.set_subtitle("CLAHE on luminance (tile-based, clip-limited)");
    contrast_row.set_active(settings.borrow().image.auto_contrast);
    tone_group.add(&contrast_row);
    let levels_row = adw::SwitchRow::new();
    levels_row.set_title("Auto levels");
    levels_row.set_subtitle("Set black and white points for each page");
    levels_row.set_active(settings.borrow().image.auto_levels);
    tone_group.add(&levels_row);
    page.add(&tone_group);

    for (mode, check) in &checks {
        let mode = *mode;
        check.connect_toggled({
            let settings = settings.clone();
            let rerender = rerender.clone();
            move |check| {
                let current = settings.borrow().image.scaling;
                let Some(scaling) = scaling_write(mode, check.is_active(), current) else {
                    return;
                };
                settings.borrow_mut().image.scaling = scaling;
                rerender();
            }
        });
    }

    contrast_row.connect_active_notify({
        let settings = settings.clone();
        let rerender = rerender.clone();
        move |row| {
            let current = settings.borrow().image.auto_contrast;
            let Some(on) = switch_write(row.is_active(), current) else {
                return;
            };
            settings.borrow_mut().image.auto_contrast = on;
            rerender();
        }
    });

    levels_row.connect_active_notify({
        let settings = settings.clone();
        let prefs = prefs.clone();
        move |row| {
            let current = settings.borrow().image.auto_levels;
            let Some(on) = switch_write(row.is_active(), current) else {
                return;
            };
            settings.borrow_mut().image.auto_levels = on;
            prefs.set_auto_levels(on);
            rerender();
        }
    });
}

/// The reading mode the two Reading radios show. Exactly one is on in
/// normal use; if both or neither are on (for example while GTK moves the
/// selection from one to the other), "Single page" wins.
fn reading_mode_from_radios(single_on: bool, two_page_on: bool) -> ReadingMode {
    if two_page_on && !single_on {
        ReadingMode::TwoPageSpreads
    } else {
        ReadingMode::SinglePage
    }
}

/// The reading mode to store after a Reading radio emits `toggled`, or
/// `None` when it matches `current`.
fn reading_write(single_on: bool, two_page_on: bool, current: ReadingMode) -> Option<ReadingMode> {
    let mode = reading_mode_from_radios(single_on, two_page_on);
    (mode != current).then_some(mode)
}

/// The scaling mode to store when the radio button for `row` emits `toggled`.
///
/// Selecting a row emits `toggled` on the previous button (inactive) and then
/// on the new one (active); only the second one writes, and only once.
fn scaling_write(row: ScalingMode, now_active: bool, current: ScalingMode) -> Option<ScalingMode> {
    (now_active && row != current).then_some(row)
}

/// The value to store when a switch row reports `on`, or `None` when it
/// matches `current`.
fn switch_write(on: bool, current: bool) -> Option<bool> {
    (on != current).then_some(on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ImageProcessingSettings;

    const SCALING: [ScalingMode; 3] = [
        ScalingMode::Nothing,
        ScalingMode::Lanczos3,
        ScalingMode::MitchellNetravali,
    ];

    const READING: [ReadingMode; 2] = [ReadingMode::SinglePage, ReadingMode::TwoPageSpreads];

    fn image(scaling: ScalingMode, auto_contrast: bool) -> ImageProcessingSettings {
        ImageProcessingSettings {
            scaling,
            auto_contrast,
            auto_levels: false,
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

    /// Feeds the radio states seen at each `toggled` signal to the Reading
    /// handler logic and returns every value it would write.
    fn reading_writes(start: ReadingMode, states: &[(bool, bool)]) -> Vec<ReadingMode> {
        let mut current = start;
        let mut writes = Vec::new();
        for &(single, two_page) in states {
            if let Some(mode) = reading_write(single, two_page, current) {
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
        if let Some(on) = switch_write(contrast_on, s.auto_contrast) {
            s.auto_contrast = on;
        }
        (s.scaling, s.auto_contrast)
    }

    #[test]
    fn reading_radios_map_to_the_reading_mode() {
        assert_eq!(
            reading_mode_from_radios(true, false),
            ReadingMode::SinglePage
        );
        assert_eq!(
            reading_mode_from_radios(false, true),
            ReadingMode::TwoPageSpreads
        );
    }

    #[test]
    fn single_page_wins_when_both_or_neither_radio_is_on() {
        assert_eq!(
            reading_mode_from_radios(true, true),
            ReadingMode::SinglePage
        );
        assert_eq!(
            reading_mode_from_radios(false, false),
            ReadingMode::SinglePage
        );
        assert_eq!(
            reading_write(true, true, ReadingMode::TwoPageSpreads),
            Some(ReadingMode::SinglePage)
        );
        assert_eq!(reading_write(false, false, ReadingMode::SinglePage), None);
    }

    #[test]
    fn reading_selection_writes_once_in_either_signal_order() {
        // Old button off first, then the new one on.
        assert_eq!(
            reading_writes(ReadingMode::SinglePage, &[(false, false), (false, true)]),
            vec![ReadingMode::TwoPageSpreads]
        );
        assert_eq!(
            reading_writes(
                ReadingMode::TwoPageSpreads,
                &[(false, false), (true, false)]
            ),
            vec![ReadingMode::SinglePage]
        );
        // New button on first, then the old one off.
        assert_eq!(
            reading_writes(ReadingMode::SinglePage, &[(true, true), (false, true)]),
            vec![ReadingMode::TwoPageSpreads]
        );
        assert_eq!(
            reading_writes(ReadingMode::TwoPageSpreads, &[(true, true), (true, false)]),
            vec![ReadingMode::SinglePage]
        );
    }

    #[test]
    fn reading_radios_write_what_the_old_switch_wrote() {
        // Old switch off wrote SinglePage, on wrote TwoPageSpreads.
        for current in READING {
            let single = reading_write(true, false, current).unwrap_or(current);
            let two_page = reading_write(false, true, current).unwrap_or(current);
            assert_eq!(single, ReadingMode::SinglePage);
            assert_eq!(two_page, ReadingMode::TwoPageSpreads);
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
        for mode in READING {
            let single = mode == ReadingMode::SinglePage;
            assert_eq!(reading_write(single, !single, mode), None);
        }
        for on in [false, true] {
            assert_eq!(switch_write(on, on), None);
        }
    }

    #[test]
    fn switches_write_only_on_change() {
        assert_eq!(switch_write(true, false), Some(true));
        assert_eq!(switch_write(false, true), Some(false));
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
}
