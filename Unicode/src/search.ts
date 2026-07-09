import type { CharInfo } from "./types";

/**
 * Case-insensitive substring search across character names.
 * Returns matches sorted by codepoint.
 */
export function searchByName(
  data: Map<number, CharInfo>,
  term: string,
): CharInfo[] {
  const lower = term.toLowerCase();
  const results: CharInfo[] = [];

  for (const info of data.values()) {
    if (info.name.toLowerCase().includes(lower)) {
      results.push(info);
    }
  }

  return results.sort((a, b) => a.codepoint - b.codepoint);
}
