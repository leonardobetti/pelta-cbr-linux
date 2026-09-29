project(
  'pelta-linux-gnome',
  'rust',
  version: '0.1.6',
  meson_version: '>= 0.59',
  license: 'AGPL-3.0-or-later',
)

gnome = import('gnome')

base_id = 'com.pelta.ComicReader'
application_id = base_id

dependency('glib-2.0', version: '>= 2.80')
dependency('gio-2.0', version: '>= 2.80')
dependency('gtk4', version: '>= 4.22')
dependency('libarchive', version: '>= 3.0')
dependency('libadwaita-1', version: '>= 1.9')

cargo = find_program('cargo', required: true)
desktop_file_validate = find_program('desktop-file-validate', required: false)
appstreamcli = find_program('appstreamcli', required: false)

version = meson.project_version()

prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
datadir = prefix / get_option('datadir')
iconsdir = datadir / 'icons'

if get_option('profile') == 'development'
  message('Building in debug mode')
  cargo_release_option = []
  rust_target = 'debug'
else
  message('Building in release mode')
  cargo_release_option = ['--release']
  rust_target = 'release'
endif

cargo_home = meson.project_build_root() / 'cargo-home'
cargo_target = meson.project_build_root() / 'cargo-target'
cargo_env = environment()
cargo_env.set('CARGO_HOME', cargo_home)
cargo_env.set('CARGO_TARGET_DIR', cargo_target)

meson.add_devenv(cargo_env)

subdir('data')
subdir('src')

gnome.post_install(
  glib_compile_schemas: true,
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
