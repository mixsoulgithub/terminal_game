## 1.1.0 — 2026-07-09

- Interactive TUI browser (`unicode` or `unicode --browse`)
  - Grid auto-sizes to terminal dimensions (not fixed 16×16)
  - Arrow keys to navigate cells, PgUp/PgDn to flip pages, Home/End for plane boundaries
  - Detail panel shows name, category, block, decomposition, case mappings
  - `g` to jump to a codepoint, `/` to search by name, `q` to quit
  - Mouse wheel support for page scrolling

## 1.0.0 — 2026-07-09

Initial release.

- Character lookup by literal character, U+XXXX, 0xXX, or decimal codepoint
- Case-insensitive name search (`--search`/`-s`)
- Displays: name, codepoint (U+/dec), category, block, bidi class, UTF-8/UTF-16 encoding, decomposition, case mappings, numeric value, Unicode 1.0 name
- Fetches and caches UnicodeData.txt + Blocks.txt from unicode.org on first run
