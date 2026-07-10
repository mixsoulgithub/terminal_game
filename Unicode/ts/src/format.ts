import type { CharInfo } from "./types";
import { categoryName } from "./types";

function hex(cp: number): string {
  return cp.toString(16).toUpperCase().padStart(4, "0");
}

function utf8(cp: number): string {
  return [...new TextEncoder().encode(String.fromCodePoint(cp))]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join(" ");
}

function utf16(cp: number): string {
  if (cp <= 0xffff) return hex(cp);
  const hi = ((cp - 0x10000) >> 10) + 0xd800;
  const lo = ((cp - 0x10000) & 0x3ff) + 0xdc00;
  return `${hex(hi)} ${hex(lo)}`;
}

function escapeChar(char: string): string {
  if (char === "\x00") return "␀";
  if (char === "\n") return "␊";
  if (char === "\t") return "␉";
  if (char === "\r") return "␍";
  if (char === " ") return "␠";
  const cp = char.codePointAt(0)!;
  if (cp < 0x20 || (cp >= 0x7f && cp <= 0x9f)) {
    return `\\x${cp.toString(16).padStart(2, "0")}`;
  }
  return char;
}

function bold(s: string): string {
  return `\x1b[1m${s}\x1b[22m`;
}

function dim(s: string): string {
  return `\x1b[2m${s}\x1b[22m`;
}

export function printChar(info: CharInfo): void {
  const char = String.fromCodePoint(info.codepoint);
  const display = escapeChar(char);

  console.log(`  ${bold("Char")}      ${display}`);
  console.log(`  ${bold("Name")}      ${info.name}`);
  console.log(
    `  ${bold("Codepoint")}  U+${hex(info.codepoint)}  (${info.codepoint} decimal)`,
  );
  console.log(`  ${bold("Category")}  ${info.category} — ${categoryName(info.category)}`);
  console.log(`  ${bold("Block")}     ${info.block}`);
  console.log(`  ${bold("Bidi")}      ${info.bidiClass}${info.bidiMirrored ? " (mirrored)" : ""}`);
  console.log(`  ${bold("UTF-8")}     ${utf8(info.codepoint)}`);
  console.log(`  ${bold("UTF-16")}    ${utf16(info.codepoint)}`);

  if (info.decomposition) {
    console.log(`  ${bold("Decomp")}    ${info.decomposition}`);
  }
  if (info.uppercaseMapping) {
    const up = parseInt(info.uppercaseMapping, 16);
    console.log(
      `  ${bold("Upper")}      U+${hex(up)}  ${String.fromCodePoint(up)}`,
    );
  }
  if (info.lowercaseMapping) {
    const lo = parseInt(info.lowercaseMapping, 16);
    console.log(
      `  ${bold("Lower")}      U+${hex(lo)}  ${String.fromCodePoint(lo)}`,
    );
  }
  if (info.titlecaseMapping) {
    const ti = parseInt(info.titlecaseMapping, 16);
    console.log(
      `  ${bold("Title")}      U+${hex(ti)}  ${String.fromCodePoint(ti)}`,
    );
  }
  if (info.numericValue) {
    console.log(`  ${bold("Numeric")}    ${info.numericValue}`);
  }
  if (info.oldName) {
    console.log(`  ${bold("Old name")}  ${info.oldName}`);
  }
}

export function printSearchResults(
  results: CharInfo[],
  term: string,
): void {
  if (results.length === 0) {
    console.log(`No characters found matching "${term}".`);
    return;
  }

  console.log(
    `${results.length} match${results.length === 1 ? "" : "es"} for "${term}":`,
  );
  console.log();

  for (const info of results.slice(0, 50)) {
    const char = escapeChar(String.fromCodePoint(info.codepoint));
    const h = hex(info.codepoint);
    console.log(
      `  ${dim(`U+${h}`)}  ${char}  ${info.name}`,
    );
  }

  if (results.length > 50) {
    console.log();
    console.log(
      dim(`  ... and ${results.length - 50} more matches`),
    );
  }
}
