//! Comic archive listing and on-demand page reads via libarchive.

use std::cmp::Ordering;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

/// Largest encoded page accepted from an archive. Comic pages are typically a
/// few megabytes; this is a zip-bomb ceiling, not a quality target.
pub const MAX_PAGE_BYTES: usize = 64 * 1024 * 1024;

/// Largest number of image entries listed from one archive.
pub const MAX_PAGES: usize = 10_000;

const ARCHIVE_OK: c_int = 0;
const BLOCK_SIZE: usize = 10_240;
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "avif", "jxl"];

/// Failure while opening an archive or reading a page.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ArchiveError {
    /// Path is not a regular file.
    #[error("file does not exist: {0}")]
    MissingFile(PathBuf),
    /// Path contained an interior NUL byte.
    #[error("invalid path: {0}")]
    InvalidPath(#[from] std::ffi::NulError),
    /// `archive_read_new` returned null.
    #[error("failed to create libarchive reader")]
    ReaderCreate,
    /// libarchive reported a failure.
    #[error("{0}")]
    Libarchive(String),
    /// Archive opened but contained no supported page images.
    #[error("no supported comic pages found in archive")]
    NoPages,
    /// Requested page index is past the listed pages.
    #[error("page index out of bounds: {0}")]
    PageOutOfBounds(usize),
    /// Listed page name was not found on a second pass through the archive.
    #[error("page not found in archive: {0}")]
    PageMissing(String),
    /// Encoded page is larger than [`MAX_PAGE_BYTES`].
    #[error("comic page exceeds {max} bytes")]
    PageTooLarge {
        /// Configured ceiling, in bytes.
        max: usize,
    },
    /// Archive listed more image entries than [`MAX_PAGES`].
    #[error("archive has too many pages (max {max})")]
    TooManyPages {
        /// Configured ceiling, in pages.
        max: usize,
    },
}

#[repr(C)]
struct ArchiveOpaque {
    _private: [u8; 0],
}

#[repr(C)]
struct ArchiveEntryOpaque {
    _private: [u8; 0],
}

extern "C" {
    fn archive_read_new() -> *mut ArchiveOpaque;
    fn archive_read_support_filter_all(a: *mut ArchiveOpaque) -> c_int;
    fn archive_read_support_format_all(a: *mut ArchiveOpaque) -> c_int;
    fn archive_read_open_filename(
        a: *mut ArchiveOpaque,
        filename: *const c_char,
        block_size: usize,
    ) -> c_int;
    fn archive_read_next_header(
        a: *mut ArchiveOpaque,
        entry: *mut *mut ArchiveEntryOpaque,
    ) -> c_int;
    fn archive_entry_pathname(entry: *mut ArchiveEntryOpaque) -> *const c_char;
    fn archive_entry_size(entry: *mut ArchiveEntryOpaque) -> i64;
    fn archive_read_data(a: *mut ArchiveOpaque, buff: *mut c_void, len: usize) -> isize;
    fn archive_read_free(a: *mut ArchiveOpaque) -> c_int;
    fn archive_error_string(a: *mut ArchiveOpaque) -> *const c_char;
}

/// Owned libarchive reader. `Drop` always calls `archive_read_free`.
struct ArchiveReader {
    ptr: NonNull<ArchiveOpaque>,
}

impl ArchiveReader {
    fn open(path: &Path) -> Result<Self, ArchiveError> {
        let c_path = CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: archive_read_new allocates a new reader or returns null.
        let raw = unsafe { archive_read_new() };
        let ptr = NonNull::new(raw).ok_or(ArchiveError::ReaderCreate)?;
        let reader = Self { ptr };

        // SAFETY: `ptr` is a live reader from archive_read_new, exclusively owned.
        let _ = unsafe { archive_read_support_filter_all(reader.as_ptr()) };
        // SAFETY: same as above; enabling all formats is the documented setup.
        let _ = unsafe { archive_read_support_format_all(reader.as_ptr()) };
        // SAFETY: `c_path` is a valid C string that lives for this call; `ptr` is live.
        let ret =
            unsafe { archive_read_open_filename(reader.as_ptr(), c_path.as_ptr(), BLOCK_SIZE) };
        if ret != ARCHIVE_OK {
            return Err(reader.libarchive_error("failed to open archive"));
        }
        Ok(reader)
    }

    fn as_ptr(&self) -> *mut ArchiveOpaque {
        self.ptr.as_ptr()
    }

    fn libarchive_error(&self, fallback: &str) -> ArchiveError {
        ArchiveError::Libarchive(self.error_string().unwrap_or_else(|| fallback.to_owned()))
    }

    fn error_string(&self) -> Option<String> {
        // SAFETY: `ptr` is a live reader; a null error string means "no message".
        let err_ptr = unsafe { archive_error_string(self.as_ptr()) };
        if err_ptr.is_null() {
            return None;
        }
        // SAFETY: libarchive documents a non-null error string as a NUL-terminated C string
        // owned by the reader, valid until the next libarchive call on this handle.
        Some(
            unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn next_header(&mut self) -> Option<NonNull<ArchiveEntryOpaque>> {
        let mut entry: *mut ArchiveEntryOpaque = std::ptr::null_mut();
        // SAFETY: `ptr` is a live open reader; `entry` is a valid out-pointer.
        let ret = unsafe { archive_read_next_header(self.as_ptr(), &raw mut entry) };
        if ret != ARCHIVE_OK {
            return None;
        }
        NonNull::new(entry)
    }

    fn pathname(entry: NonNull<ArchiveEntryOpaque>) -> Option<String> {
        // SAFETY: `entry` came from archive_read_next_header on this reader.
        let path_ptr = unsafe { archive_entry_pathname(entry.as_ptr()) };
        if path_ptr.is_null() {
            return None;
        }
        // SAFETY: non-null pathname is a NUL-terminated C string owned by the entry,
        // valid until the next header is read.
        Some(
            unsafe { CStr::from_ptr(path_ptr) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn read_current_file(
        &mut self,
        entry: NonNull<ArchiveEntryOpaque>,
    ) -> Result<Vec<u8>, ArchiveError> {
        // SAFETY: `entry` is the current header of this reader.
        let size = unsafe { archive_entry_size(entry.as_ptr()) };
        if size > i64::try_from(MAX_PAGE_BYTES).unwrap_or(i64::MAX) {
            return Err(ArchiveError::PageTooLarge {
                max: MAX_PAGE_BYTES,
            });
        }
        let mut buf = if size > 0 {
            Vec::with_capacity(usize::try_from(size).unwrap_or(0))
        } else {
            Vec::new()
        };
        let mut chunk = [0u8; 16_384];
        loop {
            // SAFETY: `chunk` is a valid writable buffer; `ptr` is the live reader.
            let n = unsafe {
                archive_read_data(
                    self.as_ptr(),
                    chunk.as_mut_ptr().cast::<c_void>(),
                    chunk.len(),
                )
            };
            if n < 0 {
                return Err(self.libarchive_error("error reading page data"));
            }
            if n == 0 {
                break;
            }
            let n = usize::try_from(n).unwrap_or(0);
            if buf.len().saturating_add(n) > MAX_PAGE_BYTES {
                return Err(ArchiveError::PageTooLarge {
                    max: MAX_PAGE_BYTES,
                });
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Ok(buf)
    }
}

impl Drop for ArchiveReader {
    fn drop(&mut self) {
        // SAFETY: `ptr` is exclusively owned and has not been freed yet.
        let _ = unsafe { archive_read_free(self.as_ptr()) };
    }
}

fn is_junk(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.contains("__macosx") || lower.contains("thumbs.db") || lower.contains(".ds_store") {
        return true;
    }
    for part in lower.split('/') {
        if part.starts_with('.') {
            return true;
        }
    }
    false
}

fn is_image(name: &str) -> bool {
    if is_junk(name) || name.ends_with('/') {
        return false;
    }
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    IMAGE_EXTS.contains(&ext.as_str())
}

/// Case-insensitive natural order so page_2 comes before page_10.
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let a_lower = a.to_ascii_lowercase();
    let b_lower = b.to_ascii_lowercase();
    let mut a_chars = a_lower.chars().peekable();
    let mut b_chars = b_lower.chars().peekable();

    loop {
        match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) if ac.is_ascii_digit() && bc.is_ascii_digit() => {
                let mut a_digits = String::new();
                while let Some(c) = a_chars.peek().copied() {
                    if c.is_ascii_digit() {
                        a_digits.push(c);
                        a_chars.next();
                    } else {
                        break;
                    }
                }
                let mut b_digits = String::new();
                while let Some(c) = b_chars.peek().copied() {
                    if c.is_ascii_digit() {
                        b_digits.push(c);
                        b_chars.next();
                    } else {
                        break;
                    }
                }

                let a_val: u128 = a_digits.parse().unwrap_or(0);
                let b_val: u128 = b_digits.parse().unwrap_or(0);
                match a_val.cmp(&b_val) {
                    Ordering::Equal => match a_digits.len().cmp(&b_digits.len()) {
                        Ordering::Equal => continue,
                        other => return other,
                    },
                    other => return other,
                }
            }
            (Some(ac), Some(bc)) => {
                a_chars.next();
                b_chars.next();
                match ac.cmp(&bc) {
                    Ordering::Equal => continue,
                    other => return other,
                }
            }
        }
    }
}

/// An opened comic archive: listed page names plus the file they came from.
#[derive(Clone, Debug)]
pub struct ComicArchive {
    path: PathBuf,
    title: String,
    pages: Vec<String>,
}

impl ComicArchive {
    /// Lists image entries in `path` (CBR / CBZ / CBT / ZIP / RAR / TAR).
    ///
    /// # Errors
    ///
    /// Returns [`ArchiveError`] if the file is missing, libarchive cannot open
    /// it, no supported pages are found, or the page list exceeds [`MAX_PAGES`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ArchiveError> {
        let path = path.as_ref();
        if !path.is_file() {
            return Err(ArchiveError::MissingFile(path.to_path_buf()));
        }

        let mut reader = ArchiveReader::open(path)?;
        let mut pages = Vec::new();
        while let Some(entry) = reader.next_header() {
            let Some(name) = ArchiveReader::pathname(entry) else {
                continue;
            };
            if is_image(&name) {
                if pages.len() >= MAX_PAGES {
                    return Err(ArchiveError::TooManyPages { max: MAX_PAGES });
                }
                pages.push(name);
            }
        }

        if pages.is_empty() {
            return Err(ArchiveError::NoPages);
        }

        pages.sort_by(|a, b| natural_cmp(a, b));

        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Comic")
            .to_owned();

        Ok(Self {
            path: path.to_path_buf(),
            title,
            pages,
        })
    }

    /// Filesystem path of the archive.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Display title, from the file stem.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Number of listed pages.
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Listed page paths in natural order.
    #[must_use]
    pub fn pages(&self) -> &[String] {
        &self.pages
    }

    /// Reads the encoded bytes of page `index`.
    ///
    /// Re-opens the archive and scans until the listed name is found. Encoded
    /// size is capped at [`MAX_PAGE_BYTES`].
    ///
    /// # Errors
    ///
    /// Returns [`ArchiveError`] if the index is out of range, the archive
    /// cannot be reopened, the page is missing, or the encoded page is larger
    /// than [`MAX_PAGE_BYTES`].
    pub fn read_page(&self, index: usize) -> Result<Vec<u8>, ArchiveError> {
        let target = self
            .pages
            .get(index)
            .ok_or(ArchiveError::PageOutOfBounds(index))?;

        let mut reader = ArchiveReader::open(&self.path)?;
        while let Some(entry) = reader.next_header() {
            let Some(name) = ArchiveReader::pathname(entry) else {
                continue;
            };
            if name == *target {
                return reader.read_current_file(entry);
            }
        }
        Err(ArchiveError::PageMissing(target.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_cbz() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/archive/tiny.cbz")
    }

    #[test]
    fn test_natural_sort() {
        let mut list = vec![
            "page_10.jpg".to_string(),
            "page_2.jpg".to_string(),
            "page_1.jpg".to_string(),
            "page_20.jpg".to_string(),
        ];
        list.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            list,
            vec!["page_1.jpg", "page_2.jpg", "page_10.jpg", "page_20.jpg"]
        );
    }

    #[test]
    fn open_missing_file_is_an_error() {
        let err = ComicArchive::open("/no/such/comic.cbz").unwrap_err();
        assert!(matches!(err, ArchiveError::MissingFile(_)), "{err}");
    }

    #[test]
    fn open_fixture_filters_junk_and_sorts_naturally() {
        let comic = ComicArchive::open(fixture_cbz()).expect("open tiny.cbz");
        assert_eq!(comic.page_count(), 2);
        assert_eq!(comic.pages(), ["page_2.png", "page_10.png"]);
        assert_eq!(comic.title(), "tiny");

        let page0 = comic.read_page(0).expect("read page 0");
        let page1 = comic.read_page(1).expect("read page 1");
        assert_ne!(page0.len(), 0, "page 0 encoded bytes");
        assert_ne!(page1.len(), 0, "page 1 encoded bytes");
        assert_eq!(&page0[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&page1[..8], b"\x89PNG\r\n\x1a\n");
        assert_ne!(page0, page1);

        let err = comic.read_page(99).unwrap_err();
        assert!(matches!(err, ArchiveError::PageOutOfBounds(99)), "{err}");
    }
}
