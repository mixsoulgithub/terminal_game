/** Parsed from UnicodeData.txt */
export interface CharInfo {
  codepoint: number;
  name: string;
  category: string;
  combiningClass: number;
  bidiClass: string;
  decomposition: string;
  numericDecimal: string;
  numericDigit: string;
  numericValue: string;
  bidiMirrored: boolean;
  oldName: string;
  isoComment: string;
  uppercaseMapping: string;
  lowercaseMapping: string;
  titlecaseMapping: string;
  block: string;
}

/** Human-readable category names */
const CATEGORY_NAMES: Record<string, string> = {
  Lu: "Uppercase Letter",
  Ll: "Lowercase Letter",
  Lt: "Titlecase Letter",
  Lm: "Modifier Letter",
  Lo: "Other Letter",
  Mn: "Nonspacing Mark",
  Mc: "Spacing Mark",
  Me: "Enclosing Mark",
  Nd: "Decimal Number",
  Nl: "Letter Number",
  No: "Other Number",
  Pc: "Connector Punctuation",
  Pd: "Dash Punctuation",
  Ps: "Open Punctuation",
  Pe: "Close Punctuation",
  Pi: "Initial Punctuation",
  Pf: "Final Punctuation",
  Po: "Other Punctuation",
  Sm: "Math Symbol",
  Sc: "Currency Symbol",
  Sk: "Modifier Symbol",
  So: "Other Symbol",
  Zs: "Space Separator",
  Zl: "Line Separator",
  Zp: "Paragraph Separator",
  Cc: "Control",
  Cf: "Format",
  Cs: "Surrogate",
  Co: "Private Use",
  Cn: "Unassigned",
};

export function categoryName(abbr: string): string {
  return CATEGORY_NAMES[abbr] ?? abbr;
}
