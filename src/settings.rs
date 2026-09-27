//! In-memory application settings.
//!
//! Kept separate from the UI so more preference groups can be added later
//! (and optionally persisted via GSettings without rewriting the reader).

/// How pages are laid out while reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadingMode {
    /// One page at a time (default).
    #[default]
    SinglePage,
    /// Facing pages as spreads; cover and back cover stay alone.
    TwoPageSpreads,
}

impl ReadingMode {
    pub fn is_two_page(self) -> bool {
        matches!(self, Self::TwoPageSpreads)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReadingSettings {
    pub mode: ReadingMode,
}

/// How page bitmaps are resampled before display.
///
/// Exactly one scaling choice is active. Defaults to [`ScalingMode::Nothing`]
/// so GTK scales the texture (current behaviour).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScalingMode {
    /// Leave decoding alone; GTK scales the texture.
    #[default]
    Nothing,
    /// Linear-light Lanczos3 resize to display size (experimental).
    Lanczos3,
    /// Linear-light Mitchell–Netravali resize to display size.
    MitchellNetravali,
}

impl ScalingMode {
    pub fn needs_custom_resize(self) -> bool {
        !matches!(self, Self::Nothing)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ImageProcessingSettings {
    pub scaling: ScalingMode,
    /// CLAHE on luminance. Mutually exclusive with Lanczos3 / Mitchell in the UI
    /// (exactly one of Nothing, Lanczos3, Mitchell, or Auto contrast is on).
    pub auto_contrast: bool,
}

impl ImageProcessingSettings {
    /// True when we must decode → process → MemoryTexture instead of GTK scale.
    pub fn needs_processing(self) -> bool {
        self.scaling.needs_custom_resize() || self.auto_contrast
    }

    /// Optional soak-test overrides: `PELTA_LANCZOS3=1`, `PELTA_MITCHELL=1`,
    /// or `PELTA_AUTO_CONTRAST=1`. Defaults remain Nothing when unset.
    /// Exclusive priority: Lanczos → Mitchell → Auto contrast.
    pub fn from_env() -> Self {
        let mut s = Self::default();
        if std::env::var_os("PELTA_LANCZOS3").is_some() {
            s.scaling = ScalingMode::Lanczos3;
            s.auto_contrast = false;
        } else if std::env::var_os("PELTA_MITCHELL").is_some() {
            s.scaling = ScalingMode::MitchellNetravali;
            s.auto_contrast = false;
        } else if std::env::var_os("PELTA_AUTO_CONTRAST").is_some() {
            s.scaling = ScalingMode::Nothing;
            s.auto_contrast = true;
        }
        s
    }
}

/// Top-level settings container. Add further groups here as needed.
#[derive(Debug, Clone, Default)]
pub struct AppSettings {
    pub reading: ReadingSettings,
    pub image: ImageProcessingSettings,
}
