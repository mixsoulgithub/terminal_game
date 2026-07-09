import blessed from "blessed";
import type { CharInfo } from "./types";
import { categoryName } from "./types";
import { loadData } from "./data";

const COLS = 32;
const ROWS = 32;
const PAGE_SIZE = COLS * ROWS;

const CAT_COLORS: Record<string, string> = {
  Lu: "yellow", Ll: "blue", Lt: "magenta", Lm: "green", Lo: "cyan",
  Mn: "red", Mc: "red", Me: "red",
  Nd: "green", Nl: "green", No: "green",
  Pc: "yellow", Pd: "yellow", Ps: "yellow", Pe: "yellow", Pi: "yellow", Pf: "yellow", Po: "yellow",
  Sm: "red", Sc: "green", Sk: "cyan", So: "magenta",
  Zs: "white", Zl: "white", Zp: "white",
  Cc: "black", Cf: "black", Cs: "black", Co: "magenta", Cn: "black",
};

let data: Map<number, CharInfo>;

function isVisible(info: CharInfo | undefined): boolean {
  if (!info) return false;
  // Not printable: control, format, surrogate, unassigned
  // Whitespace: space/line/paragraph separators
  // Zero-width: nonspacing marks
  const inv: string[] = ["Cc", "Cf", "Cs", "Cn", "Zs", "Zl", "Zp", "Mn"];
  return !inv.includes(info.category);
}

function safeGlyph(cp: number, info: CharInfo | undefined): string {
  if (!isVisible(info)) return " ";
  try { return String.fromCodePoint(cp); } catch { return " "; }
}

function ansiFg(n: number): string { return `\x1b[38;5;${n}m`; }
function ansiBg(n: number): string { return `\x1b[48;5;${n}m`; }
const ANSI_RESET = "\x1b[0m";
const ANSI_BOLD = "\x1b[1m";
const ANSI_REV = "\x1b[7m";

// Named color → ANSI 256-color index
const ANSI_COLORS: Record<string, number> = {
  yellow: 226, blue: 33, magenta: 201, green: 46, cyan: 51,
  red: 196, white: 252, black: 235,
};

function drawGrid(box: blessed.Widgets.BoxElement, base: number, sel: number): void {
  const rows: string[] = [];
  // Column header
  let hdr = "     ";
  for (let c = 0; c < COLS; c++) hdr += ` ${ANSI_BOLD}${c.toString(16).toUpperCase()}${ANSI_RESET}`;
  rows.push(hdr);
  for (let r = 0; r < ROWS; r++) {
    let line = ` ${ANSI_BOLD}${r.toString(16).toUpperCase()}${ANSI_RESET} `;
    for (let c = 0; c < COLS; c++) {
      const cp = base + r * COLS + c;
      if (cp > 0x10ffff) { line += "  "; continue; }
      const info = data.get(cp);
      const g = safeGlyph(cp, info);
      if (cp === sel) {
        line += `${ANSI_REV}${g} ${ANSI_RESET}`;
      } else {
        const ci = ANSI_COLORS[CAT_COLORS[info?.category ?? "Cn"] ?? "white"] ?? 252;
        line += `${ansiFg(ci)}${g} ${ANSI_RESET}`;
      }
    }
    rows.push(line);
  }
  box.setContent(rows.join("\n"));
}

function drawDetail(box: blessed.Widgets.BoxElement, cp: number): void {
  const info = data.get(cp);
  if (!info) {
    box.setContent(`  U+${cp.toString(16).toUpperCase().padStart(4, "0")}  —  unassigned`);
    return;
  }
  const g = safeGlyph(cp, info);
  const h = cp.toString(16).toUpperCase().padStart(4, "0");
  const B = ANSI_BOLD;
  const R = ANSI_RESET;
  const lines: string[] = [
    `  ${B}U+${h}${R}  ${g}  ${info.name}`,
    `  Category: ${B}${info.category}${R} — ${categoryName(info.category)}    Block: ${info.block}`,
  ];
  const ex: string[] = [];
  if (info.decomposition) ex.push(`Decomp: ${info.decomposition}`);
  if (info.uppercaseMapping) ex.push(`Upper: U+${parseInt(info.uppercaseMapping, 16).toString(16).toUpperCase().padStart(4, "0")}`);
  if (info.lowercaseMapping) ex.push(`Lower: U+${parseInt(info.lowercaseMapping, 16).toString(16).toUpperCase().padStart(4, "0")}`);
  if (info.numericValue) ex.push(`Numeric: ${info.numericValue}`);
  if (info.bidiMirrored) ex.push("Bidi Mirrored");
  if (ex.length > 0) lines.push(`  ${ex.join("  │  ")}`);
  box.setContent(lines.join("\n"));
}

function pageOf(cp: number): number { return Math.floor(cp / PAGE_SIZE) * PAGE_SIZE; }

export async function startTui(): Promise<void> {
  data = await loadData();

  const screen = blessed.screen({
    smartCSR: false, title: "Unicode Browser",
    cursor: { shape: "block", blink: true, artificial: false, color: "white" },
    fullUnicode: true,
  } as blessed.Widgets.IScreenOptions);

  const gridBox = blessed.box({
    top: 0, left: 0, width: "100%", height: ROWS + 1,
    tags: false, style: { fg: "white", bg: "black" },
  });
  const detailBox = blessed.box({
    top: ROWS + 1, left: 0, width: "100%", height: 3,
    tags: false, border: { type: "line" as const },
    style: { fg: "white", bg: "black", border: { fg: "cyan" } },
  });
  const statusBar = blessed.box({
    bottom: 0, left: 0, width: "100%", height: 1,
    content: ` ${ansiBg(51)}${ansiFg(0)} arrows:nav  PgUp/Dn:page  g:goto  /:search  q:quit ${ANSI_RESET}`,
    tags: false,
  });

  screen.append(gridBox);
  screen.append(detailBox);
  screen.append(statusBar);

  let base = 0;
  let sel = 0;
  const refresh = () => { drawGrid(gridBox, base, sel); drawDetail(detailBox, sel); screen.render(); };
  const jump = (cp: number) => { base = pageOf(cp); sel = cp; refresh(); };
  const move = (dr: number, dc: number) => {
    const r = Math.floor((sel - base) / COLS) + dr;
    const c = ((sel - base) % COLS) + dc;
    if (r >= 0 && r < ROWS && c >= 0 && c < COLS) {
      const ncp = base + r * COLS + c;
      if (ncp <= 0x10ffff) { sel = ncp; refresh(); }
    }
  };

  type KH = blessed.Widgets.Events.IKeyEventArg;

  screen.key(["left","right","up","down"], (_: string, k: KH) => {
    if (k.name === "left") move(0, -1);
    else if (k.name === "right") move(0, 1);
    else if (k.name === "up") move(-1, 0);
    else if (k.name === "down") move(1, 0);
  });

  screen.key(["pageup","pagedown"], (_: string, k: KH) => {
    if (k.name === "pageup") base = Math.max(0, base - PAGE_SIZE);
    else if (base + PAGE_SIZE < 0x110000) base += PAGE_SIZE;
    sel = base; refresh();
  });

  screen.key(["home","end"], (_: string, k: KH) => {
    const plane = Math.floor(base / 0x10000) * 0x10000;
    if (k.name === "home") jump(plane);
    else jump(pageOf(Math.min(plane + 0xffff, 0x10ffff)));
  });

  screen.key(["q","C-c"], () => { screen.destroy(); process.exit(0); });

  screen.key(["g"], () => {
    const p = blessed.prompt({
      parent: screen, top: "center", left: "center", height: "shrink", width: "shrink",
      border: { type: "line" as const }, keys: true, vi: true, mouse: true, tags: true,
    });
    screen.render();
    p.input("Go to (U+XXXX or decimal): ", "", (_: unknown, v: string) => {
      if (!v) { screen.render(); return; }
      const um = v.match(/^U\+([0-9A-Fa-f]{1,6})$/);
      if (um) jump(parseInt(um[1]!, 16));
      else if (/^[0-9]+$/.test(v) && parseInt(v, 10) <= 0x10ffff) jump(parseInt(v, 10));
      screen.render();
    });
  });

  screen.key(["/"], () => {
    const p = blessed.prompt({
      parent: screen, top: "center", left: "center", height: "shrink", width: "shrink",
      border: { type: "line" as const }, keys: true, vi: true, mouse: true, tags: true,
    });
    screen.render();
    p.input("Search name: ", "", (_: unknown, v: string) => {
      if (!v) { screen.render(); return; }
      const lo = v.toLowerCase();
      for (const info of data.values()) {
        if (info.name.toLowerCase().includes(lo)) { jump(info.codepoint); break; }
      }
      screen.render();
    });
  });

  screen.on("mouse", (md: { action: string }) => {
    if (md.action === "wheeldown" && base + PAGE_SIZE < 0x110000) base += PAGE_SIZE;
    else if (md.action === "wheelup") base = Math.max(0, base - PAGE_SIZE);
    else return;
    sel = base; refresh();
  });

  refresh();

  await new Promise<void>(() => {});
}
