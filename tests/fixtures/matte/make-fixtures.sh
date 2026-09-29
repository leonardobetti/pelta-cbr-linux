#!/bin/sh
# Regenerate the page matte fixtures used by src/matte.rs tests from two
# public-domain Winsor McCay pages on Wikimedia Commons:
#   https://commons.wikimedia.org/wiki/File:Little_Nemo_1907-09-29.jpg
#   https://commons.wikimedia.org/wiki/File:Little_Nemo_1905-10-15.jpg
# Needs ImageMagick 7 and curl.
set -eu
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

curl -fsSL -o "$tmp/1907.jpg" https://upload.wikimedia.org/wikipedia/commons/d/df/Little_Nemo_1907-09-29.jpg
curl -fsSL -o "$tmp/1905.jpg" https://upload.wikimedia.org/wikipedia/commons/3/38/Little_Nemo_1905-10-15.jpg

out() { magick "$1" -colorspace sRGB $2 -strip -quality 90 "$3"; }

# Whole page with a cream paper margin.
out "$tmp/1907.jpg" "-resize 480x" nemo-1907-page.jpg
# Tightly cropped page: coloured panels run into the edges.
out "$tmp/1905.jpg" "-resize 480x" nemo-1905-page.jpg
# Busy full-bleed art from inside the 1907 panels.
out "$tmp/1907.jpg" "-crop 1300x1500+450+1300 +repage -resize 320x" nemo-1907-full-bleed.jpg
# Grainy, uneven dark night-sky panel from the 1905 page.
out "$tmp/1905.jpg" "-crop 400x330+460+1990 +repage -resize 320x" nemo-1905-dark-panel.jpg
