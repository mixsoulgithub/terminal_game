// 各章地图的金标准生成器:用参考实现(refs/slay-the-cli)的 generateMap 掷出
// 第二、三章的地图排版,落成 tools/golden/e2e/act_maps.json,给
// src/core/replay.rs 的 act_maps_match_reference 测试逐行对比.
//
//   bun tools/gen_act_maps.ts
//
// 第一章的地图比对在 src/core/golden.rs(旧的 fixture 路径),这里只补二、三章,
// 以及"绿钥匙到手后不再标燃烧精英"这一个开关的两种取值.

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { generateMap, mapToString } from "/home/mix/projects/terminal_game/refs/slay-the-cli/src/engine/run/mapGen.ts";

const HERE = dirname(new URL(import.meta.url).pathname);
const SEEDS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 42, 54, 12345];

// 每行一条:`seed<TAB>act<TAB>set_burning<TAB>地图(15 行,行间用 | 连接)`
// 这样 Rust 侧不用引入 JSON 解析.
const lines: string[] = [
  "# 由 tools/gen_act_maps.ts 生成:参考实现 generateMap 的第二、三章地图排版",
  "# seed\tact\tset_burning\tburning_x\tburning_y\tburning_buff\tmap(行用 | 连接)",
];
for (const seed of SEEDS) {
  for (const act of [2, 3]) {
    for (const setBurning of [true, false]) {
      const gm = generateMap(BigInt(seed), 0, act, setBurning);
      const map = mapToString(gm);
      lines.push(
        `${seed}\t${act}\t${setBurning ? 1 : 0}\t${gm.burningEliteX}\t${gm.burningEliteY}\t${gm.burningEliteBuff}\t${map.split("\n").join("|")}`,
      );
    }
  }
}

const out = join(HERE, "golden", "e2e", "act_maps.tsv");
writeFileSync(out, lines.join("\n") + "\n");
console.log(`wrote tools/golden/e2e/act_maps.tsv (${lines.length - 2} cases)`);
