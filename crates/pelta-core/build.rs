//! Link libarchive for the safe archive wrapper.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-link-lib=archive");
}
