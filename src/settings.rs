//! Application settings: in-memory reading and image options, plus
//! [`Preferences`] persisted through GSettings.

use std::cell::Cell;

use gio::prelude::*;

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
    /// CLAHE on luminance. Independent of the scaling choice in the UI; when
    /// both are on, CLAHE runs after the resize.
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

pub const PAGE_MATTE_KEY: &str = "page-matte-detection";

/// Preferences that survive restarts, stored in GSettings under the app id.
///
/// When the schema is not installed (for example `cargo run` from a checkout)
/// the values live in memory for the session, starting from the schema
/// defaults, so the app still runs.
pub enum Preferences {
    Stored(gio::Settings),
    Session(Cell<bool>),
}

/// Must match the `<default>` in `data/com.pelta.ComicReader.gschema.xml`.
const PAGE_MATTE_DEFAULT: bool = true;

impl Preferences {
    pub fn load(schema_id: &str) -> Self {
        let installed =
            gio::SettingsSchemaSource::default().and_then(|source| source.lookup(schema_id, true));
        match installed {
            Some(schema) => Self::with_schema(&schema, None),
            None => {
                eprintln!(
                    "[pelta-linux-gnome] GSettings schema {schema_id} not installed; \
                     preferences will not be saved"
                );
                Self::Session(Cell::new(PAGE_MATTE_DEFAULT))
            }
        }
    }

    pub fn with_schema(
        schema: &gio::SettingsSchema,
        backend: Option<&gio::SettingsBackend>,
    ) -> Self {
        Self::Stored(gio::Settings::new_full(schema, backend, None))
    }

    pub fn page_matte(&self) -> bool {
        match self {
            Self::Stored(s) => s.boolean(PAGE_MATTE_KEY),
            Self::Session(v) => v.get(),
        }
    }

    pub fn set_page_matte(&self, enabled: bool) {
        match self {
            Self::Stored(s) => {
                if let Err(e) = s.set_boolean(PAGE_MATTE_KEY, enabled) {
                    eprintln!("[pelta-linux-gnome] could not save {PAGE_MATTE_KEY}: {e}");
                }
            }
            Self::Session(v) => v.set(enabled),
        }
    }
}

#[cfg(test)]
const SCHEMA_XML: &str = include_str!("../data/com.pelta.ComicReader.gschema.xml");

/// Writes the schema into `dir` and compiles it there.
#[cfg(test)]
fn compile_schema(dir: &std::path::Path) -> Result<gio::SettingsSchemaSource, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("com.pelta.ComicReader.gschema.xml"), SCHEMA_XML)
        .map_err(|e| e.to_string())?;
    let status = std::process::Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg(dir)
        .status()
        .map_err(|e| format!("glib-compile-schemas: {e}"))?;
    if !status.success() {
        return Err(format!("glib-compile-schemas failed: {status}"));
    }
    gio::SettingsSchemaSource::from_directory(dir, None, false).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const SCHEMA_ID: &str = "com.pelta.ComicReader";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pelta-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn keyfile_prefs(schema: &gio::SettingsSchema, file: &std::path::Path) -> Preferences {
        let backend = gio::keyfile_settings_backend_new(file.to_str().unwrap(), "/", None);
        Preferences::with_schema(schema, Some(&backend))
    }

    fn schema(dir: &std::path::Path) -> gio::SettingsSchema {
        compile_schema(&dir.join("schemas"))
            .unwrap()
            .lookup(SCHEMA_ID, false)
            .unwrap()
    }

    #[test]
    fn page_matte_defaults_to_on() {
        let dir = scratch("default");
        let schema = schema(&dir);
        let key = schema.key(PAGE_MATTE_KEY);
        assert_eq!(key.default_value().get::<bool>(), Some(true));
        // An install upgraded from a version without the key has nothing stored.
        let prefs = keyfile_prefs(&schema, &dir.join("settings.ini"));
        assert!(prefs.page_matte());
    }

    #[test]
    fn page_matte_toggles_and_persists() {
        let dir = scratch("persist");
        let schema = schema(&dir);
        let file = dir.join("settings.ini");

        let prefs = keyfile_prefs(&schema, &file);
        prefs.set_page_matte(false);
        assert!(!prefs.page_matte());
        gio::Settings::sync();
        drop(prefs);

        let reopened = keyfile_prefs(&schema, &file);
        assert!(!reopened.page_matte());
        reopened.set_page_matte(true);
        gio::Settings::sync();
        drop(reopened);

        assert!(keyfile_prefs(&schema, &file).page_matte());
        let stored = std::fs::read_to_string(&file).unwrap();
        assert!(stored.contains("page-matte-detection=true"), "{stored}");
    }

    #[test]
    fn session_fallback_matches_schema_default() {
        let dir = scratch("fallback");
        let default = schema(&dir)
            .key(PAGE_MATTE_KEY)
            .default_value()
            .get::<bool>();
        assert_eq!(default, Some(PAGE_MATTE_DEFAULT));

        let prefs = Preferences::Session(Cell::new(PAGE_MATTE_DEFAULT));
        assert!(prefs.page_matte());
        prefs.set_page_matte(false);
        assert!(!prefs.page_matte());
    }
}
