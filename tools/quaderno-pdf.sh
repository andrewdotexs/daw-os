#!/bin/sh
# quaderno-pdf.sh — the quaderno (docs/quaderno) as PDF files.
#
# One PDF for every chapter, and one with all of them, in
# docs/quaderno/pdf. Every chapter goes from Markdown to HTML with
# pandoc, and from HTML to PDF with a Chromium run without a window.
# The links between chapters become links between the PDF files, or,
# in the file with all the chapters, links inside it.
#
# Usage: tools/quaderno-pdf.sh
#
# Needs pandoc and a Chromium or a Chrome. CHROMIUM names the program
# when it is not one of those looked for below.
#
# The figures are not drawn here: tools/quaderno-figures.py does that.

set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
quaderno="${here}/docs/quaderno"
out="${quaderno}/pdf"
chapters="README 01-il-quadro 02-i-concetti 03-le-prove 04-la-storia 05-errori-e-stime 06-glossario"

die() {
	echo "quaderno-pdf: $*" >&2
	exit 1
}

command -v pandoc > /dev/null || die "pandoc is not installed"
chromium=${CHROMIUM:-}
if [ -z "${chromium}" ]; then
	for candidate in chromium chromium-browser google-chrome /opt/pw-browsers/chromium; do
		if command -v "${candidate}" > /dev/null; then
			chromium=${candidate}
			break
		fi
	done
fi
[ -n "${chromium}" ] || die "no Chromium found: set CHROMIUM to the program"

work=$(mktemp -d)
trap 'rm -rf "${work}"' EXIT

cat > "${work}/style.css" <<'CSS'
@page { size: A4; margin: 18mm 16mm 20mm 16mm; @bottom-center { content: counter(page); font: 8pt system-ui, sans-serif; color: #666; } }
html { font: 10.5pt/1.45 system-ui, -apple-system, "Segoe UI", Helvetica, Arial, sans-serif; color: #191919; }
body { margin: 0; max-width: none; padding: 0; }
h1 { font-size: 20pt; margin: 0 0 10pt; }
h2 { font-size: 14pt; margin: 18pt 0 6pt; break-after: avoid; }
h3 { font-size: 11.5pt; margin: 14pt 0 4pt; break-after: avoid; }
h1.chapter { break-before: page; }
p, li { orphans: 3; widows: 3; }
a { color: #1757ac; text-decoration: none; }
code { font: 9pt ui-monospace, Menlo, Consolas, monospace; background: #f4f4f4; padding: 0 2pt; border-radius: 2pt; }
pre { background: #f4f4f4; padding: 6pt 8pt; border-radius: 4pt; white-space: pre-wrap; word-break: break-all; break-inside: avoid; }
pre code { background: none; padding: 0; font-size: 8pt; }
table { border-collapse: collapse; width: 100%; margin: 8pt 0; font-size: 8.5pt; line-height: 1.35; }
th, td { border: 0.5pt solid #d9d9d9; padding: 3pt 5pt; text-align: left; vertical-align: top; }
th { background: #f4f4f4; }
tr { break-inside: avoid; }
td code, th code { font-size: 7.5pt; word-break: break-word; }
img { display: block; max-width: 100%; height: auto; margin: 10pt auto 4pt; break-inside: avoid; }
p:has(> img) { break-after: avoid; margin-bottom: 0; }
p:has(> img) + p { font-size: 9pt; color: #444; margin-top: 2pt; }
CSS

# The identifier pandoc gives the first heading of chapter $1.
heading_id() {
	sed -n '1s/^# //p' "${quaderno}/$1.md" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9 -]//g; s/ /-/g'
}

# From HTML file $1 to PDF file $2.
to_pdf() {
	"${chromium}" --headless --no-sandbox --disable-gpu --no-pdf-header-footer \
		--print-to-pdf="$2" "file://$1" > "${work}/chromium.log" 2>&1 ||
		die "Chromium failed on $1: $(tail -n 3 "${work}/chromium.log")"
}

# From Markdown on standard input to HTML file $1, with the title $2.
to_html() {
	pandoc --from gfm --to html5 --standalone --embed-resources \
		--resource-path "${quaderno}" --css "${work}/style.css" \
		--metadata "pagetitle=$2" --metadata lang=it --output "$1"
}

mkdir -p "${out}"

# ---- One file for every chapter: a link to another chapter is a link
# to its file.
for chapter in ${chapters}; do
	sed -E 's/\]\(([0-9A-Za-z-]+)\.md(#[^)]*)?\)/](\1.pdf)/g' "${quaderno}/${chapter}.md" |
		to_html "${work}/${chapter}.html" "Il quaderno di dawOS"
	to_pdf "${work}/${chapter}.html" "${out}/${chapter}.pdf"
	echo "docs/quaderno/pdf/${chapter}.pdf"
done

# ---- All the chapters in one file: a link to another chapter is a
# link to its heading, or to the heading it names.
: > "${work}/all.md"
script=
for chapter in ${chapters}; do
	script="${script}s/\\]\\(${chapter}\\.md\\)/](#$(heading_id "${chapter}"))/g;"
done
for chapter in ${chapters}; do
	sed -E "s/\\]\\([0-9A-Za-z-]+\\.md(#[^)]*)\\)/](\\1)/g;${script}" "${quaderno}/${chapter}.md" >> "${work}/all.md"
	printf '\n' >> "${work}/all.md"
done
to_html "${work}/all.html" "Il quaderno di dawOS" < "${work}/all.md"
# Every chapter on a page of its own.
sed -i -E 's/<h1 /<h1 class="chapter" /g' "${work}/all.html"
to_pdf "${work}/all.html" "${out}/quaderno-dawos.pdf"
echo "docs/quaderno/pdf/quaderno-dawos.pdf"
