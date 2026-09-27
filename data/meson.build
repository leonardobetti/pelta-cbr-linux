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
