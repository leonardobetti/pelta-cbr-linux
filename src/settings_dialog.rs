//! Settings dialog and the pure mapping between its controls and
//! [`AppSettings`](crate::settings::AppSettings).

#![cfg_attr(not(test), allow(dead_code))]

use crate::settings::{ImageProcessingSettings, ReadingMode, ScalingMode};

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
