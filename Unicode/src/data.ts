import type { CharInfo } from "./types";

const CACHE_DIR = `${Bun.env.HOME ?? "~"}/.cache/unicode-cli`;
const UCD_BASE = "https://www.unicode.org/Public/UCD/latest/ucd";
const UNICODE_DATA = "UnicodeData.txt";
const BLOCKS = "Blocks.txt";

let dataMap: Map<number, CharInfo> | null = null;

async function cachedFetch(filename: string): Promise<string> {
  const cachePath = `${CACHE_DIR}/${filename}`;
  const file = Bun.file(cachePath);

  if (await file.exists()) {
    const cached = (await file.text()).trimEnd();
    if (cached.length > 0) return cached;
  }

  const url = `${UCD_BASE}/${filename}`;
  const resp = await fetch(url);
  if (!resp.ok) {
    throw new Error(
      `Failed to fetch ${url}: ${resp.status} ${resp.statusText}`,
    );
  }
  const text = await resp.text();

  // Ensure cache dir exists
  await Bun.write(cachePath, text);

  return text;
}

function parseUnicodeData(text: string): Map<number, CharInfo> {
  const map = new Map<number, CharInfo>();

  for (const line of text.split("\n")) {
    if (!line || line.startsWith("#")) continue;

    const [
      code,
      name,
      category,
      combiningClass,
      bidiClass,
      decomposition,
      numericDecimal,
      numericDigit,
      numericValue,
      bidiMirrored,
      oldName,
      isoComment,
      uppercaseMapping,
      lowercaseMapping,
      titlecaseMapping,
    ] = line.split(";");

    if (!code || !name) continue;

    try {
      const cp = parseInt(code, 16);
      if (isNaN(cp)) continue;

      // Skip noncharacters and surrogates for cleaner results
      map.set(cp, {
        codepoint: cp,
        name: name.trim(),
        category: category?.trim() ?? "",
        combiningClass: parseInt(combiningClass ?? "0") || 0,
        bidiClass: bidiClass?.trim() ?? "",
        decomposition: decomposition?.trim() ?? "",
        numericDecimal: numericDecimal?.trim() ?? "",
        numericDigit: numericDigit?.trim() ?? "",
        numericValue: numericValue?.trim() ?? "",
        bidiMirrored: (bidiMirrored?.trim() ?? "N") === "Y",
        oldName: oldName?.trim() ?? "",
        isoComment: isoComment?.trim() ?? "",
        uppercaseMapping: uppercaseMapping?.trim() ?? "",
        lowercaseMapping: lowercaseMapping?.trim() ?? "",
        titlecaseMapping: titlecaseMapping?.trim() ?? "",
        block: "Unknown",
      });
    } catch {
      // skip malformed entries
    }
  }

  return map;
}

function parseBlocks(text: string): Map<number, string> {
  const blocks = new Map<number, string>();

  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;

    const match = trimmed.match(/^([0-9A-Fa-f]+)\.\.([0-9A-Fa-f]+);\s*(.+)$/);
    if (!match) continue;

    const start = parseInt(match[1]!, 16);
    const end = parseInt(match[2]!, 16);
    const name = match[3]!.trim();

    blocks.set(start, name);
    // Store end so we can find which block a codepoint belongs to
    blocks.set(-(end + 1), name);
  }

  return blocks;
}

function assignBlocks(
  chars: Map<number, CharInfo>,
  blocksText: string,
): void {
  const blocks = parseBlocks(blocksText);

  // Build sorted block starts for efficient lookup
  const starts = Array.from(blocks.entries())
    .filter(([k]) => k >= 0)
    .sort(([a], [b]) => a - b);

  for (const [cp, info] of chars) {
    // Binary search for the block
    let lo = 0;
    let hi = starts.length - 1;
    let found = "Unknown";

    while (lo <= hi) {
      const mid = (lo + hi) >>> 1;
      const [blockStart, blockName] = starts[mid]!;
      if (cp >= blockStart) {
        found = blockName;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }

    info.block = found;
  }
}

export async function loadData(): Promise<Map<number, CharInfo>> {
  if (dataMap) return dataMap;

  const [unicodeText, blocksText] = await Promise.all([
    cachedFetch(UNICODE_DATA),
    cachedFetch(BLOCKS),
  ]);

  dataMap = parseUnicodeData(unicodeText);
  assignBlocks(dataMap, blocksText);

  return dataMap;
}

export function getData(): Map<number, CharInfo> | null {
  return dataMap;
}
