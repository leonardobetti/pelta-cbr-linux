binary_name = meson.project_name()

cargo_build_options = [
  '--manifest-path',
  meson.project_source_root() / 'Cargo.toml',
]
cargo_build_options += cargo_release_option

cargo_script = find_program(
  meson.project_source_root() / 'build-aux' / 'cargo-build.sh',
)

custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: binary_name,
  console: true,
  install: true,
  install_dir: bindir,
  env: cargo_env,
  command: [
    cargo_script,
    cargo,
    binary_name,
    rust_target,
    cargo_target,
    '@OUTPUT@',
    '--',
  ] + cargo_build_options,
)

# Offline: flatpak-builder runs tests without network, after the build step
# has already fetched every crate into CARGO_HOME.
test(
  'cargo-test',
  cargo,
  args: ['test', '--offline'] + cargo_build_options,
  env: cargo_env,
  timeout: 1800,
  is_parallel: false,
)
