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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ImageProcessingSettings {
    pub scaling: ScalingMode,
    /// CLAHE on luminance. Independent of the scaling choice in the UI; when
    /// both are on, CLAHE runs after the resize.
    pub auto_contrast: bool,
    /// Black and white points per page, after the resize and before CLAHE.
    /// Stored in GSettings ([`AUTO_LEVELS_KEY`]).
    pub auto_levels: bool,
}

impl ImageProcessingSettings {
    /// True when we must decode → process → MemoryTexture instead of GTK scale.
    pub fn needs_processing(self) -> bool {
        self.scaling.needs_custom_resize() || self.auto_contrast || self.auto_levels
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
pub const AUTO_LEVELS_KEY: &str = "auto-levels-enabled";

/// Preferences that survive restarts, stored in GSettings under the app id.
///
/// When the schema is not installed (for example `cargo run` from a checkout)
/// the values live in memory for the session, starting from the schema
/// defaults, so the app still runs.
pub enum Preferences {
    Stored(gio::Settings),
    Session {
        page_matte: Cell<bool>,
        auto_levels: Cell<bool>,
    },
}

/// Must match the `<default>` values in `data/com.pelta.ComicReader.gschema.xml`.
const PAGE_MATTE_DEFAULT: bool = true;
const AUTO_LEVELS_DEFAULT: bool = false;

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
                Self::session()
            }
        }
    }

    pub fn with_schema(
        schema: &gio::SettingsSchema,
        backend: Option<&gio::SettingsBackend>,
    ) -> Self {
        Self::Stored(gio::Settings::new_full(schema, backend, None))
    }

    fn session() -> Self {
        Self::Session {
            page_matte: Cell::new(PAGE_MATTE_DEFAULT),
            auto_levels: Cell::new(AUTO_LEVELS_DEFAULT),
        }
    }

    pub fn page_matte(&self) -> bool {
        self.boolean(PAGE_MATTE_KEY)
    }

    pub fn set_page_matte(&self, enabled: bool) {
        self.set_boolean(PAGE_MATTE_KEY, enabled);
    }

    pub fn auto_levels(&self) -> bool {
        self.boolean(AUTO_LEVELS_KEY)
    }

    pub fn set_auto_levels(&self, enabled: bool) {
        self.set_boolean(AUTO_LEVELS_KEY, enabled);
    }

    fn session_cell(&self, key: &str) -> Option<&Cell<bool>> {
        match (self, key) {
            (Self::Session { page_matte, .. }, PAGE_MATTE_KEY) => Some(page_matte),
            (Self::Session { auto_levels, .. }, AUTO_LEVELS_KEY) => Some(auto_levels),
            _ => None,
        }
    }

    fn boolean(&self, key: &str) -> bool {
        match self {
            Self::Stored(s) => s.boolean(key),
            Self::Session { .. } => self.session_cell(key).is_some_and(Cell::get),
        }
    }

    fn set_boolean(&self, key: &str, enabled: bool) {
        match self {
            Self::Stored(s) => {
                if let Err(e) = s.set_boolean(key, enabled) {
                    eprintln!("[pelta-linux-gnome] could not save {key}: {e}");
                }
            }
            Self::Session { .. } => {
                if let Some(cell) = self.session_cell(key) {
                    cell.set(enabled);
                }
            }
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

        let prefs = Preferences::session();
        assert!(prefs.page_matte());
        prefs.set_page_matte(false);
        assert!(!prefs.page_matte());
    }

    #[test]
    fn auto_levels_defaults_to_off() {
        let dir = scratch("levels-default");
        let schema = schema(&dir);
        let key = schema.key(AUTO_LEVELS_KEY);
        assert_eq!(key.default_value().get::<bool>(), Some(false));
        assert_eq!(key.default_value().get::<bool>(), Some(AUTO_LEVELS_DEFAULT));
        let prefs = keyfile_prefs(&schema, &dir.join("settings.ini"));
        assert!(!prefs.auto_levels());
        assert!(!Preferences::session().auto_levels());
    }

    #[test]
    fn auto_levels_persists_independently_of_page_matte() {
        let dir = scratch("levels-persist");
        let schema = schema(&dir);
        let file = dir.join("settings.ini");

        let prefs = keyfile_prefs(&schema, &file);
        prefs.set_auto_levels(true);
        assert!(prefs.auto_levels());
        assert!(prefs.page_matte());
        gio::Settings::sync();
        drop(prefs);

        let reopened = keyfile_prefs(&schema, &file);
        assert!(reopened.auto_levels());
        assert!(reopened.page_matte());
        let stored = std::fs::read_to_string(&file).unwrap();
        assert!(stored.contains("auto-levels-enabled=true"), "{stored}");

        let session = Preferences::session();
        session.set_auto_levels(true);
        assert!(session.auto_levels());
        assert!(session.page_matte());
    }

    /// The schema may gain only this one key over 0.1.7.
    #[test]
    fn schema_has_exactly_the_two_expected_keys() {
        let dir = scratch("keys");
        let mut keys: Vec<String> = schema(&dir)
            .list_keys()
            .iter()
            .map(|k| k.to_string())
            .collect();
        keys.sort();
        assert_eq!(keys, [AUTO_LEVELS_KEY, PAGE_MATTE_KEY]);
    }
}
