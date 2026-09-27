use std::cmp::Ordering;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[repr(C)]
struct ArchiveOpaque {
    _private: [u8; 0],
}

#[repr(C)]
struct ArchiveEntryOpaque {
    _private: [u8; 0],
}

const ARCHIVE_OK: c_int = 0;

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

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "avif", "jxl"];

fn is_junk(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.contains("__macosx") || lower.contains("thumbs.db") || lower.contains(".ds_store") {
        return true;
    }
    // AppleDouble or hidden
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
    let ext = name.split('.').last().unwrap_or("").to_ascii_lowercase();
    IMAGE_EXTS.contains(&ext.as_str())
}

/// Case-insensitive natural order so page_2 comes before page_10.
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

#[derive(Clone, Debug)]
pub struct ComicArchive {
    pub path: PathBuf,
    pub title: String,
    pub pages: Vec<String>,
}

impl ComicArchive {
    pub fn open(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Err(format!("File does not exist: {}", path.display()));
        }

        let c_path =
            CString::new(path.as_os_str().as_bytes()).map_err(|e| format!("Invalid path: {e}"))?;

        let a = unsafe { archive_read_new() };
        if a.is_null() {
            return Err("Failed to create libarchive reader".into());
        }

        unsafe {
            archive_read_support_filter_all(a);
            archive_read_support_format_all(a);
            let ret = archive_read_open_filename(a, c_path.as_ptr(), 10240);
            if ret != ARCHIVE_OK {
                let err_ptr = archive_error_string(a);
                let msg = if !err_ptr.is_null() {
                    CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
                } else {
                    "Failed to open archive".into()
                };
                archive_read_free(a);
                return Err(msg);
            }
        }

        let mut pages = Vec::new();
        let mut entry: *mut ArchiveEntryOpaque = std::ptr::null_mut();

        while unsafe { archive_read_next_header(a, &mut entry) } == ARCHIVE_OK {
            let path_ptr = unsafe { archive_entry_pathname(entry) };
            if path_ptr.is_null() {
                continue;
            }
            let name = unsafe { CStr::from_ptr(path_ptr).to_string_lossy().into_owned() };
            if is_image(&name) {
                pages.push(name);
            }
        }

        unsafe {
            archive_read_free(a);
        }

        if pages.is_empty() {
            return Err("No supported comic pages found in archive".into());
        }

        pages.sort_by(|a, b| natural_cmp(a, b));

        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Comic")
            .to_string();

        Ok(ComicArchive {
            path: path.to_path_buf(),
            title,
            pages,
        })
    }

    pub fn read_page(&self, index: usize) -> Result<Vec<u8>, String> {
        let target = self
            .pages
            .get(index)
            .ok_or_else(|| format!("Page index out of bounds: {index}"))?;

        let c_path = CString::new(self.path.as_os_str().as_bytes())
            .map_err(|e| format!("Invalid path: {e}"))?;

        let a = unsafe { archive_read_new() };
        if a.is_null() {
            return Err("Failed to create libarchive reader".into());
        }

        unsafe {
            archive_read_support_filter_all(a);
            archive_read_support_format_all(a);
            let ret = archive_read_open_filename(a, c_path.as_ptr(), 10240);
            if ret != ARCHIVE_OK {
                let err_ptr = archive_error_string(a);
                let msg = if !err_ptr.is_null() {
                    CStr::from_ptr(err_ptr).to_string_lossy().into_owned()
                } else {
                    "Failed to open archive".into()
                };
                archive_read_free(a);
                return Err(msg);
            }
        }

        let mut entry: *mut ArchiveEntryOpaque = std::ptr::null_mut();
        let mut found_data = None;

        while unsafe { archive_read_next_header(a, &mut entry) } == ARCHIVE_OK {
            let path_ptr = unsafe { archive_entry_pathname(entry) };
            if path_ptr.is_null() {
                continue;
            }
            let name = unsafe { CStr::from_ptr(path_ptr).to_string_lossy() };
            if name == target.as_str() {
                let size = unsafe { archive_entry_size(entry) };
                let mut buf = if size > 0 {
                    Vec::with_capacity(size as usize)
                } else {
                    Vec::new()
                };
                let mut chunk = [0u8; 16384];
                loop {
                    let n = unsafe {
                        archive_read_data(a, chunk.as_mut_ptr() as *mut c_void, chunk.len())
                    };
                    if n < 0 {
                        let err_ptr = unsafe { archive_error_string(a) };
                        let msg = if !err_ptr.is_null() {
                            unsafe { CStr::from_ptr(err_ptr).to_string_lossy().into_owned() }
                        } else {
                            "Error reading page data".into()
                        };
                        unsafe {
                            archive_read_free(a);
                        }
                        return Err(msg);
                    }
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n as usize]);
                }
                found_data = Some(buf);
                break;
            }
        }

        unsafe {
            archive_read_free(a);
        }

        found_data.ok_or_else(|| format!("Page not found in archive: {target}"))
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::gdk::prelude::*;

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
    fn test_open_yazidi_cbr() {
        let path = Path::new(
            "/home/lozbek/Downloads/CBR/Yazidi! (2023) (digital) (Mr Norrell-Empire).cbr",
        );
        if !path.exists() {
            eprintln!("Skipping test_open_yazidi_cbr: file not found");
            return;
        }
        let comic = ComicArchive::open(path).expect("Failed to open Yazidi CBR");
        println!(
            "Yazidi CBR opened successfully! Title: {}, Pages: {}",
            comic.title,
            comic.page_count()
        );
        assert_eq!(comic.page_count(), 137);
        assert_eq!(comic.pages[0], "Yazidi!-0000.jpg");

        // Read page 0
        let bytes = comic.read_page(0).expect("Failed to read page 0");
        assert!(!bytes.is_empty());
        println!(
            "Read page 0: {} bytes (starts with {:?})",
            bytes.len(),
            &bytes[..4]
        );
        // JPEG magic: FF D8 FF
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF]);

        let gbytes = glib::Bytes::from(&bytes);
        let texture =
            gtk4::gdk::Texture::from_bytes(&gbytes).expect("Failed to decode texture from bytes");
        println!(
            "Texture decoded! Size: {}x{}",
            texture.width(),
            texture.height()
        );
        assert!(texture.width() > 0 && texture.height() > 0);
    }

    #[test]
    fn test_open_cbz() {
        let path = Path::new("/home/lozbek/Downloads/CBR/Rwama v01 My Childhood In Algeria (2025) (Graphic Novel) (Europe Comics) (Digital-HD) (LeDuch).cbz");
        if !path.exists() {
            eprintln!("Skipping test_open_cbz: file not found");
            return;
        }
        let comic = ComicArchive::open(path).expect("Failed to open CBZ");
        println!(
            "CBZ opened successfully! Title: {}, Pages: {}",
            comic.title,
            comic.page_count()
        );
        assert!(comic.page_count() > 0);

        let bytes = comic.read_page(0).expect("Failed to read page 0");
        assert!(!bytes.is_empty());
        println!("Read CBZ page 0: {} bytes", bytes.len());
    }
}
