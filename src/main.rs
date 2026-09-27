//! Pelta Comic Reader — Linux GNOME (GTK4 + libadwaita).
//!
//! Wayland-native only.
//! Aligned with GNOME Papers comic book backend:
//! - Reads archive entries (.cbr, .cbz) via libarchive
//! - Natural alphanumeric ordering for pages
//! - Decodes pages on demand
//! - Lets GTK scale textures to the widget

mod archive;
mod image_proc;
mod reading;
mod settings;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use adw::prelude::*;
use archive::ComicArchive;
use image_proc::{process_page, CacheKey, ProcessCache, ProcessedPage};
use reading::{can_go_next, can_go_prev, next_anchor, prev_anchor, view_for_page, PageView};
use settings::{AppSettings, ImageProcessingSettings, ReadingMode, ScalingMode};

const APP_ID: &str = "com.pelta.ComicReader";
const APP_TITLE: &str = "Pelta Comic Reader";

struct AppState {
    archive: Option<ComicArchive>,
    current_page: usize,
    last_scroll: Instant,
}

/// Set by `build_ui` so `Application::open` can load a path without racing
/// window actions (fixes "This application can not open files" / silent no-op).
type OpenFn = Rc<dyn Fn(PathBuf)>;

fn main() {
    ensure_wayland_only();

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    let opener: Rc<RefCell<Option<OpenFn>>> = Rc::new(RefCell::new(None));

    app.connect_startup(|_| {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);
    });

    app.connect_activate({
        let opener = opener.clone();
        move |app| {
            build_ui(app, &opener);
        }
    });

    app.connect_open({
        let opener = opener.clone();
        move |app, files, _| {
            build_ui(app, &opener);
            if let Some(file) = files.first() {
                if let Some(path) = file.path() {
                    if let Some(open) = opener.borrow().as_ref() {
                        open(path);
                    } else {
                        eprintln!(
                            "[pelta-linux-gnome] open requested but UI opener not ready: {}",
                            path.display()
                        );
                    }
                }
            }
        }
    });

    app.run();
}

fn make_page_picture() -> gtk4::Picture {
    let picture = gtk4::Picture::new();
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_halign(gtk4::Align::Fill);
    picture.set_valign(gtk4::Align::Fill);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture
}

fn load_texture(bytes: &[u8]) -> Result<gtk4::gdk::Texture, glib::Error> {
    let gbytes = glib::Bytes::from(bytes);
    gtk4::gdk::Texture::from_bytes(&gbytes)
}

fn texture_from_rgba(page: &ProcessedPage) -> gtk4::gdk::MemoryTexture {
    gtk4::gdk::MemoryTexture::new(
        page.width as i32,
        page.height as i32,
        gtk4::gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from(&page.rgba),
        (page.width as usize) * 4,
    )
}

/// Widget slot size for display-size processing (never full archive resolution).
fn slot_size(widget: &impl gtk4::prelude::IsA<gtk4::Widget>, fallback_w: i32, fallback_h: i32) -> (u32, u32) {
    let w = widget.as_ref().width();
    let h = widget.as_ref().height();
    let w = if w > 32 { w } else { fallback_w }.max(1) as u32;
    let h = if h > 32 { h } else { fallback_h }.max(1) as u32;
    (w, h)
}

fn apply_raw_bytes(picture: &gtk4::Picture, bytes: &[u8]) -> Result<(), String> {
    let texture = load_texture(bytes).map_err(|e| e.to_string())?;
    picture.set_paintable(Some(&texture));
    Ok(())
}

fn apply_processed_page(picture: &gtk4::Picture, page: &ProcessedPage) {
    let texture = texture_from_rgba(page);
    picture.set_paintable(Some(&texture));
}

#[derive(Clone, Copy)]
enum PictureSlot {
    Left,
    Right,
}

/// Whether `page` is part of the view for the current navigation anchor.
fn page_still_visible(
    page: usize,
    anchor: usize,
    page_count: usize,
    two_page: bool,
) -> Option<PictureSlot> {
    match view_for_page(anchor, page_count, two_page)? {
        PageView::Single(p) if p == page => Some(PictureSlot::Left),
        PageView::Spread(l, _) if l == page => Some(PictureSlot::Left),
        PageView::Spread(_, r) if r == page => Some(PictureSlot::Right),
        _ => None,
    }
}

fn neighbor_page_indices(anchor: usize, page_count: usize, two_page: bool) -> Vec<usize> {
    let mut out = Vec::new();
    let mut push_view = |a: usize| {
        if let Some(v) = view_for_page(a, page_count, two_page) {
            match v {
                PageView::Single(p) => out.push(p),
                PageView::Spread(l, r) => {
                    out.push(l);
                    out.push(r);
                }
            }
        }
    };
    if let Some(p) = prev_anchor(anchor, page_count, two_page) {
        push_view(p);
    }
    if let Some(n) = next_anchor(anchor, page_count, two_page) {
        push_view(n);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Cache-hit or background-process one visible page.
///
/// Never flashes the raw decode: keeps the current paintable until the
/// processed frame is ready (or shows a spinner when the slot is empty).
fn schedule_processed_page(
    page: usize,
    bytes: Vec<u8>,
    picture: gtk4::Picture,
    slot_w: u32,
    slot_h: u32,
    img: ImageProcessingSettings,
    cache: Rc<RefCell<ProcessCache>>,
    gen_cell: Rc<Cell<u64>>,
    my_gen: u64,
    state: Rc<RefCell<AppState>>,
    settings: Rc<RefCell<AppSettings>>,
    spinner: gtk4::Spinner,
    pending: Rc<Cell<u32>>,
    toast: adw::ToastOverlay,
) {
    let key = CacheKey {
        page,
        width: slot_w,
        height: slot_h,
        scaling: img.scaling,
        auto_contrast: img.auto_contrast,
    };
    if let Some(hit) = cache.borrow_mut().get(&key) {
        apply_processed_page(&picture, &hit);
        return;
    }

    // No raw intermediate frame — hold previous page; spinner only if nothing to show yet.
    let show_spinner = picture.paintable().is_none();
    if show_spinner {
        pending.set(pending.get().saturating_add(1));
        spinner.set_visible(true);
        spinner.start();
    }

    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || process_page(&bytes, slot_w, slot_h, img)).await;
        let finish_spinner = || {
            if !show_spinner {
                return;
            }
            let n = pending.get().saturating_sub(1);
            pending.set(n);
            if n == 0 {
                spinner.stop();
                spinner.set_visible(false);
            }
        };
        if gen_cell.get() != my_gen {
            finish_spinner();
            return;
        }
        match result {
            Ok(Ok(processed)) => {
                cache.borrow_mut().insert(key, processed.clone());
                let (anchor, count) = {
                    let st = state.borrow();
                    (
                        st.current_page,
                        st.archive.as_ref().map(|a| a.page_count()).unwrap_or(0),
                    )
                };
                let two_page = settings.borrow().reading.mode.is_two_page();
                if page_still_visible(page, anchor, count, two_page).is_some() {
                    apply_processed_page(&picture, &processed);
                }
                finish_spinner();
            }
            Ok(Err(e)) => {
                finish_spinner();
                toast.add_toast(adw::Toast::new(&format!(
                    "Image processing failed (page {}): {e}",
                    page + 1
                )));
            }
            Err(_) => {
                finish_spinner();
                toast.add_toast(adw::Toast::new(&format!(
                    "Image processing task cancelled (page {})",
                    page + 1
                )));
            }
        }
    });
}

/// Warm the LRU cache for a neighbor page without touching Picture widgets.
fn prefetch_processed_page(
    page: usize,
    bytes: Vec<u8>,
    slot_w: u32,
    slot_h: u32,
    img: ImageProcessingSettings,
    cache: Rc<RefCell<ProcessCache>>,
    gen_cell: Rc<Cell<u64>>,
    my_gen: u64,
) {
    let key = CacheKey {
        page,
        width: slot_w,
        height: slot_h,
        scaling: img.scaling,
        auto_contrast: img.auto_contrast,
    };
    if cache.borrow_mut().get(&key).is_some() {
        return;
    }
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || process_page(&bytes, slot_w, slot_h, img)).await;
        if gen_cell.get() != my_gen {
            return;
        }
        if let Ok(Ok(processed)) = result {
            cache.borrow_mut().insert(key, processed);
        }
    });
}

fn build_ui(app: &adw::Application, opener: &Rc<RefCell<Option<OpenFn>>>) {
    if let Some(window) = app.active_window() {
        window.present();
        return;
    }

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title(APP_TITLE)
        .default_width(1100)
        .default_height(800)
        .build();

    let toolbar_view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let window_title = adw::WindowTitle::new(APP_TITLE, "");
    header.set_title_widget(Some(&window_title));

    let open_btn = gtk4::Button::from_icon_name("document-open-symbolic");
    open_btn.set_tooltip_text(Some("Open Comic (Ctrl+O)"));
    header.pack_start(&open_btn);

    let settings_btn = gtk4::Button::from_icon_name("emblem-system-symbolic");
    settings_btn.set_tooltip_text(Some("Settings"));
    header.pack_start(&settings_btn);

    let prev_btn = gtk4::Button::from_icon_name("go-previous-symbolic");
    prev_btn.set_tooltip_text(Some("Previous Page (Left Arrow, Page Up)"));
    prev_btn.set_sensitive(false);
    header.pack_start(&prev_btn);

    let next_btn = gtk4::Button::from_icon_name("go-next-symbolic");
    next_btn.set_tooltip_text(Some("Next Page (Right Arrow, Page Down, Space)"));
    next_btn.set_sensitive(false);
    header.pack_start(&next_btn);

    let fullscreen_btn = gtk4::Button::from_icon_name("view-fullscreen-symbolic");
    fullscreen_btn.set_tooltip_text(Some("Toggle Fullscreen (F11)"));
    header.pack_end(&fullscreen_btn);

    toolbar_view.add_top_bar(&header);

    let toast_overlay = adw::ToastOverlay::new();

    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::Crossfade);

    let status_page = adw::StatusPage::builder()
        .icon_name("com.pelta.ComicReader")
        .title(APP_TITLE)
        .description("Open a CBR or CBZ comic archive to start reading")
        .build();

    let open_status_btn = gtk4::Button::builder()
        .label("Open Comic…")
        .halign(gtk4::Align::Center)
        .css_classes(["suggested-action", "pill"])
        .build();
    status_page.set_child(Some(&open_status_btn));
    stack.add_named(&status_page, Some("empty"));

    let reader_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    reader_box.set_halign(gtk4::Align::Fill);
    reader_box.set_valign(gtk4::Align::Fill);
    reader_box.set_hexpand(true);
    reader_box.set_vexpand(true);
    reader_box.set_margin_start(4);
    reader_box.set_margin_end(4);

    let picture_left = make_page_picture();
    let picture_right = make_page_picture();
    picture_right.set_visible(false);
    reader_box.append(&picture_left);
    reader_box.append(&picture_right);

    let reader_overlay = gtk4::Overlay::new();
    reader_overlay.set_child(Some(&reader_box));
    let process_spinner = gtk4::Spinner::new();
    process_spinner.set_halign(gtk4::Align::Center);
    process_spinner.set_valign(gtk4::Align::Center);
    process_spinner.set_size_request(48, 48);
    process_spinner.set_visible(false);
    process_spinner.set_can_target(false);
    reader_overlay.add_overlay(&process_spinner);
    stack.add_named(&reader_overlay, Some("reader"));

    toast_overlay.set_child(Some(&stack));
    toolbar_view.set_content(Some(&toast_overlay));
    window.set_content(Some(&toolbar_view));

    let state = Rc::new(RefCell::new(AppState {
        archive: None,
        current_page: 0,
        last_scroll: Instant::now(),
    }));
    let settings = Rc::new(RefCell::new(AppSettings {
        reading: Default::default(),
        image: ImageProcessingSettings::from_env(),
    }));
    let process_cache = Rc::new(RefCell::new(ProcessCache::default()));
    // Bumped only on settings / archive change so neighbor prefetch is not cancelled by page turns.
    let process_gen = Rc::new(Cell::new(0u64));
    let process_pending = Rc::new(Cell::new(0u32));

    // Helper closure to show a page / spread for the current reading mode.
    let show_page = {
        let state = state.clone();
        let settings = settings.clone();
        let process_cache = process_cache.clone();
        let process_gen = process_gen.clone();
        let process_pending = process_pending.clone();
        let process_spinner = process_spinner.clone();
        let picture_left = picture_left.clone();
        let picture_right = picture_right.clone();
        let window_title = window_title.clone();
        let window = window.clone();
        let prev_btn = prev_btn.clone();
        let next_btn = next_btn.clone();
        let stack = stack.clone();
        let toast_overlay = toast_overlay.clone();
        let reader_box = reader_box.clone();

        Rc::new(move |page_idx: usize| {
            let two_page = settings.borrow().reading.mode.is_two_page();
            let img_settings = settings.borrow().image;
            let (title, page_count, view) = {
                let st = state.borrow();
                let Some(archive) = st.archive.as_ref() else {
                    return;
                };
                let page_count = archive.page_count();
                let Some(view) = view_for_page(page_idx, page_count, two_page) else {
                    return;
                };
                (archive.title.clone(), page_count, view)
            };

            let read_page = |idx: usize| -> Result<Vec<u8>, String> {
                let st = state.borrow();
                let archive = st.archive.as_ref().ok_or_else(|| "no archive".to_string())?;
                archive.read_page(idx).map_err(|e| e.to_string())
            };

            let finish_chrome = |anchor: usize, subtitle: String| {
                state.borrow_mut().current_page = anchor;
                window_title.set_title(&title);
                window_title.set_subtitle(&subtitle);
                window.set_title(Some(&format!("{} — {}", title, APP_TITLE)));
                prev_btn.set_sensitive(can_go_prev(anchor, page_count, two_page));
                next_btn.set_sensitive(can_go_next(anchor, page_count, two_page));
                stack.set_visible_child_name("reader");
            };

            // Do not bump process_gen here — that would cancel neighbor prefetch.
            let my_gen = process_gen.get();

            let win_w = window.default_width().max(window.width()).max(800);
            let win_h = window.default_height().max(window.height()).max(600);

            let prefetch_neighbors = |anchor: usize, slot_w: u32, slot_h: u32| {
                for np in neighbor_page_indices(anchor, page_count, two_page) {
                    if let Ok(nb) = read_page(np) {
                        prefetch_processed_page(
                            np,
                            nb,
                            slot_w,
                            slot_h,
                            img_settings,
                            process_cache.clone(),
                            process_gen.clone(),
                            my_gen,
                        );
                    }
                }
            };

            match view {
                PageView::Single(p) => match read_page(p) {
                    Ok(bytes) => {
                        // Set anchor before async apply so visibility checks see the new page.
                        finish_chrome(p, format!("Page {} of {}", p + 1, page_count));
                        if img_settings.needs_processing() {
                            let (sw, sh) = slot_size(&picture_left, win_w, win_h);
                            schedule_processed_page(
                                p,
                                bytes,
                                picture_left.clone(),
                                sw,
                                sh,
                                img_settings,
                                process_cache.clone(),
                                process_gen.clone(),
                                my_gen,
                                state.clone(),
                                settings.clone(),
                                process_spinner.clone(),
                                process_pending.clone(),
                                toast_overlay.clone(),
                            );
                            prefetch_neighbors(p, sw, sh);
                        } else if let Err(e) = apply_raw_bytes(&picture_left, &bytes) {
                            toast_overlay.add_toast(adw::Toast::new(&format!(
                                "Failed to decode page {}: {e}",
                                p + 1
                            )));
                            return;
                        }
                        picture_right.set_paintable(gtk4::gdk::Paintable::NONE);
                        picture_right.set_visible(false);
                    }
                    Err(e) => {
                        toast_overlay.add_toast(adw::Toast::new(&format!(
                            "Failed to read page {}: {e}",
                            p + 1
                        )));
                    }
                },
                PageView::Spread(left, right) => {
                    let left_bytes = match read_page(left) {
                        Ok(b) => b,
                        Err(e) => {
                            toast_overlay.add_toast(adw::Toast::new(&format!(
                                "Failed to read page {}: {e}",
                                left + 1
                            )));
                            return;
                        }
                    };
                    let right_bytes = match read_page(right) {
                        Ok(b) => b,
                        Err(e) => {
                            toast_overlay.add_toast(adw::Toast::new(&format!(
                                "Failed to read page {}: {e}",
                                right + 1
                            )));
                            return;
                        }
                    };
                    finish_chrome(
                        left,
                        format!("Pages {}–{} of {}", left + 1, right + 1, page_count),
                    );
                    if img_settings.needs_processing() {
                        let box_w = reader_box.width().max(win_w);
                        let box_h = reader_box.height().max(win_h);
                        let slot_w = (box_w / 2).max(1) as u32;
                        let slot_h = box_h.max(1) as u32;
                        schedule_processed_page(
                            left,
                            left_bytes,
                            picture_left.clone(),
                            slot_w,
                            slot_h,
                            img_settings,
                            process_cache.clone(),
                            process_gen.clone(),
                            my_gen,
                            state.clone(),
                            settings.clone(),
                            process_spinner.clone(),
                            process_pending.clone(),
                            toast_overlay.clone(),
                        );
                        schedule_processed_page(
                            right,
                            right_bytes,
                            picture_right.clone(),
                            slot_w,
                            slot_h,
                            img_settings,
                            process_cache.clone(),
                            process_gen.clone(),
                            my_gen,
                            state.clone(),
                            settings.clone(),
                            process_spinner.clone(),
                            process_pending.clone(),
                            toast_overlay.clone(),
                        );
                        picture_right.set_visible(true);
                        prefetch_neighbors(left, slot_w, slot_h);
                    } else {
                        if let Err(e) = apply_raw_bytes(&picture_left, &left_bytes) {
                            toast_overlay.add_toast(adw::Toast::new(&format!(
                                "Failed to decode page {}: {e}",
                                left + 1
                            )));
                            return;
                        }
                        match load_texture(&right_bytes) {
                            Ok(texture) => {
                                picture_right.set_paintable(Some(&texture));
                                picture_right.set_visible(true);
                            }
                            Err(e) => {
                                toast_overlay.add_toast(adw::Toast::new(&format!(
                                    "Failed to decode page {}: {e}",
                                    right + 1
                                )));
                                return;
                            }
                        }
                    }
                }
            }
        })
    };

    let go_prev = {
        let state = state.clone();
        let settings = settings.clone();
        let show_page = show_page.clone();
        Rc::new(move || {
            let two_page = settings.borrow().reading.mode.is_two_page();
            let st = state.borrow();
            let cur = st.current_page;
            let count = st.archive.as_ref().map(|a| a.page_count()).unwrap_or(0);
            drop(st);
            if let Some(prev) = prev_anchor(cur, count, two_page) {
                show_page(prev);
            }
        })
    };

    let go_next = {
        let state = state.clone();
        let settings = settings.clone();
        let show_page = show_page.clone();
        Rc::new(move || {
            let two_page = settings.borrow().reading.mode.is_two_page();
            let st = state.borrow();
            let cur = st.current_page;
            let count = st.archive.as_ref().map(|a| a.page_count()).unwrap_or(0);
            drop(st);
            if let Some(next) = next_anchor(cur, count, two_page) {
                show_page(next);
            }
        })
    };

    // Open comic helper
    let open_comic = {
        let state = state.clone();
        let show_page = show_page.clone();
        let toast_overlay = toast_overlay.clone();
        let process_cache = process_cache.clone();
        let process_gen = process_gen.clone();
        let process_pending = process_pending.clone();
        let process_spinner = process_spinner.clone();

        Rc::new(move |path: PathBuf| {
            {
                let st = state.borrow();
                if let Some(archive) = st.archive.as_ref() {
                    if archive.path == path {
                        return;
                    }
                }
            }
            match ComicArchive::open(&path) {
                Ok(archive) => {
                    eprintln!(
                        "[pelta-linux-gnome] Opened '{}' with {} pages",
                        archive.title,
                        archive.page_count()
                    );
                    process_cache.borrow_mut().clear();
                    process_gen.set(process_gen.get().wrapping_add(1));
                    process_pending.set(0);
                    process_spinner.stop();
                    process_spinner.set_visible(false);
                    state.borrow_mut().archive = Some(archive);
                    show_page(0);
                }
                Err(err) => {
                    eprintln!("[pelta-linux-gnome] Failed to open comic: {err}");
                    toast_overlay.add_toast(adw::Toast::new(&format!("Error: {err}")));
                }
            }
        }) as OpenFn
    };

    // Expose opener to Application::open (CLI / file association).
    *opener.borrow_mut() = Some(open_comic.clone());

    // Action to open path (D-Bus / activate_action)
    let open_path_action = gio::SimpleAction::new("open-path", Some(&glib::VariantTy::STRING));
    open_path_action.connect_activate({
        let open_comic = open_comic.clone();
        move |_, param| {
            if let Some(val) = param.and_then(|p| p.str()) {
                open_comic(PathBuf::from(val));
            }
        }
    });
    window.add_action(&open_path_action);

    // Trigger open dialog
    let prompt_open_dialog = {
        let window = window.clone();
        let open_comic = open_comic.clone();

        move || {
            let dialog = gtk4::FileDialog::builder()
                .title("Open Comic")
                .modal(true)
                .build();

            let filter = gtk4::FileFilter::new();
            filter.set_name(Some("Comic Archives (*.cbr, *.cbz)"));
            filter.add_pattern("*.cbr");
            filter.add_pattern("*.cbz");
            filter.add_pattern("*.cbt");
            filter.add_pattern("*.CBR");
            filter.add_pattern("*.CBZ");
            filter.add_pattern("*.CBT");

            let all_filter = gtk4::FileFilter::new();
            all_filter.set_name(Some("All Files"));
            all_filter.add_pattern("*");

            let filters = gio::ListStore::new::<gtk4::FileFilter>();
            filters.append(&filter);
            filters.append(&all_filter);
            dialog.set_filters(Some(&filters));

            let open_comic = open_comic.clone();
            dialog.open(Some(&window), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        open_comic(path);
                    }
                }
            });
        }
    };

    open_btn.connect_clicked({
        let prompt = prompt_open_dialog.clone();
        move |_| prompt()
    });

    open_status_btn.connect_clicked({
        let prompt = prompt_open_dialog.clone();
        move |_| prompt()
    });

    settings_btn.connect_clicked({
        let window = window.clone();
        let settings = settings.clone();
        let show_page = show_page.clone();
        let state = state.clone();
        let process_cache = process_cache.clone();
        let process_gen = process_gen.clone();
        move |_| {
            open_settings_dialog(
                &window,
                &settings,
                &show_page,
                &state,
                &process_cache,
                &process_gen,
            );
        }
    });

    prev_btn.connect_clicked({
        let go_prev = go_prev.clone();
        move |_| go_prev()
    });

    next_btn.connect_clicked({
        let go_next = go_next.clone();
        move |_| go_next()
    });

    fullscreen_btn.connect_clicked({
        let window = window.clone();
        move |_| {
            if window.is_fullscreen() {
                window.unfullscreen();
            } else {
                window.fullscreen();
            }
        }
    });

    // Gesture click on reader: left 40% = prev, right 40% = next
    let click_gesture = gtk4::GestureClick::new();
    click_gesture.connect_pressed({
        let go_prev = go_prev.clone();
        let go_next = go_next.clone();
        let reader_box = reader_box.clone();
        move |_, _, x, _| {
            let width = reader_box.width() as f64;
            if width <= 0.0 {
                return;
            }
            let ratio = x / width;
            if ratio < 0.4 {
                go_prev();
            } else if ratio > 0.6 {
                go_next();
            }
        }
    });
    reader_box.add_controller(click_gesture);

    // Mouse scroll controller with debounce
    let scroll_controller = gtk4::EventControllerScroll::new(
        gtk4::EventControllerScrollFlags::VERTICAL | gtk4::EventControllerScrollFlags::DISCRETE,
    );
    scroll_controller.connect_scroll({
        let state = state.clone();
        let go_prev = go_prev.clone();
        let go_next = go_next.clone();
        move |_, _, dy| {
            let mut st = state.borrow_mut();
            let now = Instant::now();
            if now.duration_since(st.last_scroll).as_millis() < 200 {
                return glib::Propagation::Stop;
            }
            st.last_scroll = now;
            let has_archive = st.archive.is_some();
            drop(st);
            if !has_archive {
                return glib::Propagation::Proceed;
            }
            if dy > 0.0 {
                go_next();
            } else if dy < 0.0 {
                go_prev();
            }
            glib::Propagation::Stop
        }
    });
    window.add_controller(scroll_controller);

    // Keyboard shortcuts
    let key_controller = gtk4::EventControllerKey::new();
    key_controller.connect_key_pressed({
        let state = state.clone();
        let show_page = show_page.clone();
        let go_prev = go_prev.clone();
        let go_next = go_next.clone();
        let window = window.clone();
        let prompt_open = prompt_open_dialog.clone();

        move |_, key, _, modifier| {
            let is_ctrl = modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK);

            if is_ctrl && (key == gtk4::gdk::Key::o || key == gtk4::gdk::Key::O) {
                prompt_open();
                return glib::Propagation::Stop;
            }

            match key {
                gtk4::gdk::Key::Left
                | gtk4::gdk::Key::Page_Up
                | gtk4::gdk::Key::BackSpace
                | gtk4::gdk::Key::h
                | gtk4::gdk::Key::k => {
                    go_prev();
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::Right
                | gtk4::gdk::Key::Page_Down
                | gtk4::gdk::Key::space
                | gtk4::gdk::Key::l
                | gtk4::gdk::Key::j => {
                    go_next();
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::Home => {
                    show_page(0);
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::End => {
                    let count = state
                        .borrow()
                        .archive
                        .as_ref()
                        .map(|a| a.page_count())
                        .unwrap_or(0);
                    if count > 0 {
                        show_page(count - 1);
                    }
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::F11 | gtk4::gdk::Key::f => {
                    if window.is_fullscreen() {
                        window.unfullscreen();
                    } else {
                        window.fullscreen();
                    }
                    glib::Propagation::Stop
                }
                gtk4::gdk::Key::Escape => {
                    if window.is_fullscreen() {
                        window.unfullscreen();
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                }
                _ => glib::Propagation::Proceed,
            }
        }
    });
    window.add_controller(key_controller);

    // Drag and Drop target
    let drop_target = gtk4::DropTarget::new(gio::File::static_type(), gtk4::gdk::DragAction::COPY);
    drop_target.connect_drop({
        let open_comic = open_comic.clone();
        move |_, value, _, _| {
            if let Ok(file) = value.get::<gio::File>() {
                if let Some(path) = file.path() {
                    open_comic(path);
                    return true;
                }
            }
            false
        }
    });
    window.add_controller(drop_target);

    window.present();
}

/// Preferences dialog (not ShortcutsDialog — that widget is for keyboard
/// shortcut lists and cannot host SwitchRows / preference groups).
fn open_settings_dialog(
    window: &adw::ApplicationWindow,
    settings: &Rc<RefCell<AppSettings>>,
    show_page: &Rc<impl Fn(usize) + 'static>,
    state: &Rc<RefCell<AppState>>,
    process_cache: &Rc<RefCell<ProcessCache>>,
    process_gen: &Rc<Cell<u64>>,
) {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Settings");
    dialog.set_search_enabled(false);

    let page = adw::PreferencesPage::new();
    page.set_title("General");
    page.set_icon_name(Some("preferences-system-symbolic"));

    let reading_group = adw::PreferencesGroup::new();
    reading_group.set_title("Reading");

    let single_row = adw::SwitchRow::new();
    single_row.set_title("Single-page");
    single_row.set_subtitle("One page at a time");

    let two_row = adw::SwitchRow::new();
    two_row.set_title("Two-page spreads");
    two_row.set_subtitle("Facing pages; cover and back cover stay alone");

    {
        let mode = settings.borrow().reading.mode;
        single_row.set_active(mode == ReadingMode::SinglePage);
        two_row.set_active(mode == ReadingMode::TwoPageSpreads);
    }

    let updating = Rc::new(Cell::new(false));

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

    single_row.connect_active_notify({
        let two_row = two_row.clone();
        let settings = settings.clone();
        let show_page = show_page.clone();
        let state = state.clone();
        let updating = updating.clone();
        move |row| {
            if updating.get() {
                return;
            }
            if row.is_active() {
                updating.set(true);
                two_row.set_active(false);
                updating.set(false);
                settings.borrow_mut().reading.mode = ReadingMode::SinglePage;
                let page = state.borrow().current_page;
                if state.borrow().archive.is_some() {
                    show_page(page);
                }
            } else if !two_row.is_active() {
                // Exactly one mode must stay on.
                updating.set(true);
                row.set_active(true);
                updating.set(false);
            }
        }
    });

    two_row.connect_active_notify({
        let single_row = single_row.clone();
        let settings = settings.clone();
        let show_page = show_page.clone();
        let state = state.clone();
        let updating = updating.clone();
        move |row| {
            if updating.get() {
                return;
            }
            if row.is_active() {
                updating.set(true);
                single_row.set_active(false);
                updating.set(false);
                settings.borrow_mut().reading.mode = ReadingMode::TwoPageSpreads;
                let page = state.borrow().current_page;
                if state.borrow().archive.is_some() {
                    show_page(page);
                }
            } else if !single_row.is_active() {
                updating.set(true);
                row.set_active(true);
                updating.set(false);
            }
        }
    });

    reading_group.add(&single_row);
    reading_group.add(&two_row);
    page.add(&reading_group);

    // --- Image processing (exactly one option; default is Nothing / GTK scale) ---
    let image_group = adw::PreferencesGroup::new();
    image_group.set_title("Image processing");

    let nothing_row = adw::SwitchRow::new();
    nothing_row.set_title("Nothing (default)");
    nothing_row.set_subtitle("GTK scales the texture");

    let lanczos_row = adw::SwitchRow::new();
    lanczos_row.set_title("Lanczos3 (experimental)");
    lanczos_row.set_subtitle("Linear-light Lanczos3 resize to display size");

    let mitchell_row = adw::SwitchRow::new();
    mitchell_row.set_title("Mitchell-Netravali / Catmull-Rom");
    mitchell_row.set_subtitle("Linear-light Mitchell resize to display size");

    let contrast_row = adw::SwitchRow::new();
    contrast_row.set_title("Auto contrast");
    contrast_row.set_subtitle("CLAHE on luminance (tile-based, clip-limited)");

    {
        let img = settings.borrow().image;
        // Exactly one exclusive choice: custom scaling XOR auto-contrast XOR nothing.
        let contrast = img.auto_contrast;
        nothing_row.set_active(!contrast && img.scaling == ScalingMode::Nothing);
        lanczos_row.set_active(!contrast && img.scaling == ScalingMode::Lanczos3);
        mitchell_row.set_active(!contrast && img.scaling == ScalingMode::MitchellNetravali);
        contrast_row.set_active(contrast);
    }

    let scale_updating = Rc::new(Cell::new(false));

    nothing_row.connect_active_notify({
        let lanczos_row = lanczos_row.clone();
        let mitchell_row = mitchell_row.clone();
        let contrast_row = contrast_row.clone();
        let settings = settings.clone();
        let updating = scale_updating.clone();
        let rerender = rerender.clone();
        move |row| {
            if updating.get() {
                return;
            }
            if row.is_active() {
                updating.set(true);
                lanczos_row.set_active(false);
                mitchell_row.set_active(false);
                contrast_row.set_active(false);
                updating.set(false);
                {
                    let mut s = settings.borrow_mut();
                    s.image.scaling = ScalingMode::Nothing;
                    s.image.auto_contrast = false;
                }
                rerender();
            } else if !lanczos_row.is_active()
                && !mitchell_row.is_active()
                && !contrast_row.is_active()
            {
                updating.set(true);
                row.set_active(true);
                updating.set(false);
            }
        }
    });

    lanczos_row.connect_active_notify({
        let nothing_row = nothing_row.clone();
        let mitchell_row = mitchell_row.clone();
        let contrast_row = contrast_row.clone();
        let settings = settings.clone();
        let updating = scale_updating.clone();
        let rerender = rerender.clone();
        move |row| {
            if updating.get() {
                return;
            }
            if row.is_active() {
                updating.set(true);
                nothing_row.set_active(false);
                mitchell_row.set_active(false);
                contrast_row.set_active(false);
                updating.set(false);
                {
                    let mut s = settings.borrow_mut();
                    s.image.scaling = ScalingMode::Lanczos3;
                    s.image.auto_contrast = false;
                }
                rerender();
            } else if !nothing_row.is_active()
                && !mitchell_row.is_active()
                && !contrast_row.is_active()
            {
                updating.set(true);
                row.set_active(true);
                updating.set(false);
            }
        }
    });

    mitchell_row.connect_active_notify({
        let nothing_row = nothing_row.clone();
        let lanczos_row = lanczos_row.clone();
        let contrast_row = contrast_row.clone();
        let settings = settings.clone();
        let updating = scale_updating.clone();
        let rerender = rerender.clone();
        move |row| {
            if updating.get() {
                return;
            }
            if row.is_active() {
                updating.set(true);
                nothing_row.set_active(false);
                lanczos_row.set_active(false);
                contrast_row.set_active(false);
                updating.set(false);
                {
                    let mut s = settings.borrow_mut();
                    s.image.scaling = ScalingMode::MitchellNetravali;
                    s.image.auto_contrast = false;
                }
                rerender();
            } else if !nothing_row.is_active()
                && !lanczos_row.is_active()
                && !contrast_row.is_active()
            {
                updating.set(true);
                row.set_active(true);
                updating.set(false);
            }
        }
    });

    contrast_row.connect_active_notify({
        let nothing_row = nothing_row.clone();
        let lanczos_row = lanczos_row.clone();
        let mitchell_row = mitchell_row.clone();
        let settings = settings.clone();
        let updating = scale_updating.clone();
        let rerender = rerender.clone();
        move |row| {
            if updating.get() {
                return;
            }
            if row.is_active() {
                updating.set(true);
                nothing_row.set_active(false);
                lanczos_row.set_active(false);
                mitchell_row.set_active(false);
                updating.set(false);
                {
                    let mut s = settings.borrow_mut();
                    s.image.scaling = ScalingMode::Nothing;
                    s.image.auto_contrast = true;
                }
                rerender();
            } else if !nothing_row.is_active()
                && !lanczos_row.is_active()
                && !mitchell_row.is_active()
            {
                updating.set(true);
                row.set_active(true);
                updating.set(false);
            }
        }
    });

    image_group.add(&nothing_row);
    image_group.add(&lanczos_row);
    image_group.add(&mitchell_row);
    image_group.add(&contrast_row);
    page.add(&image_group);

    dialog.add(&page);
    dialog.present(Some(window));
}

/// Pelta's Linux shell is Wayland only. Never fall back to X11/XWayland.
fn ensure_wayland_only() {
    const MSG: &str = "Pelta requires a Wayland session; X11/XWayland is not supported.";

    let backend = std::env::var("GDK_BACKEND").unwrap_or_default();
    let backend_ok = backend.is_empty()
        || backend
            .split(',')
            .all(|b| matches!(b.trim(), "wayland" | ""));
    let wayland_display = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());

    if !backend_ok || !wayland_display {
        eprintln!("[pelta-linux-gnome] {MSG}");
        if !backend_ok {
            eprintln!("[pelta-linux-gnome] GDK_BACKEND={backend} is not allowed.");
        }
        if !wayland_display {
            eprintln!("[pelta-linux-gnome] WAYLAND_DISPLAY is not set.");
        }
        std::process::exit(1);
    }

    gtk4::gdk::set_allowed_backends("wayland");
}
