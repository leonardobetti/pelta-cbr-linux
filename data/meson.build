# Install desktop entry + AppStream + icons (Meson layer over Cargo).
# Pattern follows World/Rust/gtk-rust-template.

install_data(
  'com.pelta.ComicReader.desktop',
  install_dir: datadir / 'applications',
)

install_data(
  'com.pelta.ComicReader.metainfo.xml',
  install_dir: datadir / 'metainfo',
)

install_data(
  'com.pelta.ComicReader.gschema.xml',
  install_dir: datadir / 'glib-2.0' / 'schemas',
)

glib_compile_schemas = find_program('glib-compile-schemas', required: false)
if glib_compile_schemas.found()
  test(
    'validate-schema',
    glib_compile_schemas,
    args: ['--strict', '--dry-run', meson.current_source_dir()],
  )
endif

if desktop_file_validate.found()
  test(
    'validate-desktop',
    desktop_file_validate,
    args: [meson.current_source_dir() / 'com.pelta.ComicReader.desktop'],
  )
endif

if appstreamcli.found()
  test(
    'validate-appdata',
    appstreamcli,
    args: [
      'validate',
      '--no-net',
      '--explain',
      meson.current_source_dir() / 'com.pelta.ComicReader.metainfo.xml',
    ],
  )
endif

subdir('icons')
