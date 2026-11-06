//! Reading layout and image-processing options shared by the UI and the core pipeline.

/// How pages are laid out while reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(
    clippy::exhaustive_enums,
    reason = "single page and two-page spreads are the complete layout set"
)]
pub enum ReadingMode {
    /// One page at a time (default).
    #[default]
    SinglePage,
    /// Facing pages as spreads; cover and back cover stay alone.
    TwoPageSpreads,
}

impl ReadingMode {
    /// True when facing pages should be paired.
    #[must_use]
    pub const fn is_two_page(self) -> bool {
        matches!(self, Self::TwoPageSpreads)
    }
}

/// In-memory reading layout.
#[derive(Debug, Clone, Default)]
pub struct ReadingSettings {
    /// Single page or two-page spreads.
    pub mode: ReadingMode,
}

/// How page bitmaps are resampled before display.
///
/// Exactly one scaling choice is active. Defaults to [`ScalingMode::Nothing`]
/// so GTK scales the texture (current behaviour).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[expect(
    clippy::exhaustive_enums,
    reason = "Nothing, Lanczos3, and Mitchell are the complete scaling set"
)]
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
    /// True when a custom resizer must run instead of GTK scaling.
    #[must_use]
    pub const fn needs_custom_resize(self) -> bool {
        !matches!(self, Self::Nothing)
    }
}

/// Opt-in decode → process pipeline settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ImageProcessingSettings {
    /// Resampling algorithm.
    pub scaling: ScalingMode,
    /// CLAHE on luminance. Independent of the scaling choice in the UI; when
    /// both are on, CLAHE runs after the resize.
    pub auto_contrast: bool,
    /// Black and white points per page, after the resize and before CLAHE.
    pub auto_levels: bool,
}

impl ImageProcessingSettings {
    /// True when we must decode → process → MemoryTexture instead of GTK scale.
    #[must_use]
    pub const fn needs_processing(self) -> bool {
        self.scaling.needs_custom_resize() || self.auto_contrast || self.auto_levels
    }

    /// Optional soak-test overrides: `PELTA_LANCZOS3=1`, `PELTA_MITCHELL=1`,
    /// or `PELTA_AUTO_CONTRAST=1`. Defaults remain Nothing when unset.
    /// Exclusive priority: Lanczos → Mitchell → Auto contrast.
    #[must_use]
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
    /// Page / spread layout.
    pub reading: ReadingSettings,
    /// Decode and resampling pipeline.
    pub image: ImageProcessingSettings,
}
