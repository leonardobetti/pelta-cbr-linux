//! Forward the Meson / Flatpak application id into the GTK binary.

fn main() {
    println!("cargo:rerun-if-env-changed=PELTA_APPLICATION_ID");
    if let Ok(id) = std::env::var("PELTA_APPLICATION_ID") {
        if !id.is_empty() {
            println!("cargo:rustc-env=PELTA_APPLICATION_ID={id}");
        }
    }
}
