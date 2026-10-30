//! Shared comic-archive and image-processing core for Pelta.
//!
//! This crate has no GTK dependency. The Linux GNOME shell lives in the
//! `pelta-linux-gnome` binary.

#![doc(html_no_source)]

pub mod archive;
pub mod auto_levels;
pub mod image_proc;
pub mod matte;
pub mod reading;
pub mod settings;

pub use archive::{ArchiveError, ComicArchive};
pub use image_proc::{
    decode_rgba, process_page, CacheKey, ProcessCache, ProcessError, ProcessedPage,
};
pub use matte::{detect_page, detect_spread, Detection, Rgb};
pub use reading::{can_go_next, can_go_prev, next_anchor, prev_anchor, view_for_page, PageView};
pub use settings::{
    AppSettings, ImageProcessingSettings, ReadingMode, ReadingSettings, ScalingMode,
};
