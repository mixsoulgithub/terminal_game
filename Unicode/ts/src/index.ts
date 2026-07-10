#!/usr/bin/env bun
import { loadData } from "./data";
import { searchByName } from "./search";
import { printChar, printSearchResults } from "./format";
import { startTui } from "./tui";

function parseCodepoint(s: string): number | null {
  // U+XXXX or U+XXXXX
  const uMatch = s.match(/^U\+([0-9A-Fa-f]{4,6})$/);
  if (uMatch) return parseInt(uMatch[1]!, 16);

  // 0xXXXX
  const hexMatch = s.match(/^0x([0-9A-Fa-f]+)$/);
  if (hexMatch) return parseInt(hexMatch[1]!, 16);

  // Decimal
  if (/^\d+$/.test(s)) {
    const n = parseInt(s, 10);
    if (n >= 0 && n <= 0x10ffff) return n;
  }

  return null;
}

function showHelp(): void {
  console.log(`\
Usage:
  unicode                    Launch interactive TUI browser
  unicode <query>            Quick character lookup
  unicode --search <term>    Search characters by name

  Look up a character or codepoint:
    unicode A          Character 'A'
    unicode 😀         Emoji character
    unicode U+0041     By U+ notation
    unicode 0x41       By hex notation
    unicode 65         By decimal

  Options:
    --browse, -b       Launch TUI browser (default with no args)
    --search, -s       Search characters by name
    --help, -h         Show this help`);
}
async function main(): Promise<void> {
  const args = process.argv.slice(2);

  if (args.length === 0 || args[0] === "--browse" || args[0] === "-b") {
    await startTui();
    process.exit(0);
  }

  // Handle flags
  if (args[0] === "--help" || args[0] === "-h") {
    showHelp();
    process.exit(0);
  }

  if (args[0] === "--search" || args[0] === "-s") {
    const term = args[1];
    if (!term) {
      console.error("Error: --search requires a search term.");
      process.exit(1);
    }

    const data = await loadData();
    const results = searchByName(data, term);
    printSearchResults(results, term);
    process.exit(0);
  }

  // Single query: character or codepoint
  const query = args[0]!;

  // Try codepoint notation first
  const cp = parseCodepoint(query);
  if (cp !== null) {
    const data = await loadData();
    const info = data.get(cp);
    if (!info) {
      console.error(`Error: no character found for codepoint U+${cp.toString(16).toUpperCase().padStart(4, "0")}.`);
      process.exit(1);
    }
    printChar(info);
    process.exit(0);
  }

  // Treat as character(s): take the first codepoint
  const charCp = query.codePointAt(0);
  if (charCp == null) {
    console.error("Error: invalid query.");
    process.exit(1);
  }

  const data = await loadData();
  const info = data.get(charCp);
  if (!info) {
    console.error(
      `Error: no character found for U+${charCp.toString(16).toUpperCase().padStart(4, "0")}.`,
    );
    process.exit(1);
  }

  // If query has more than one codepoint, print char + show the first
  const display = [...query].slice(0, 10).join(" ");
  if ([...query].length > 10) {
    console.log(`Showing first of ${[...query].length} characters: ${display}...`);
  } else if ([...query].length > 1) {
    console.log(`Input "${query}" → showing first character '${display[0]}':`);
  }

  printChar(info);
}

main().catch((err) => {
  console.error(`Error: ${err.message ?? err}`);
  process.exit(1);
});
