// 用参考实现自己的 markup 解析器(../refs/slay-the-cli/src/cli/text/markup.ts)
// 把语料解析一遍, 输出给 tools/check_corpus.py 逐字段对账。
// 跑法: bun run slay-the-spire/tools/corpus_check.ts
import { resolveMarkup } from "../../refs/slay-the-cli/src/cli/text/markup";

const root = new URL("../../refs/slay-the-cli/data/corpus/", import.meta.url);
const read = async (name: string) =>
  JSON.parse(await Bun.file(new URL(name + ".json", root)).text());
// 一行一条, 用 tab 分隔, 换行和制表符转义掉
const flat = (s: string) => s.replace(/\\/g, "\\\\").replace(/\t/g, " ").replace(/\n/g, "\\n");

for (const c of await read("cards")) {
  const meta = JSON.stringify({
    color: c.color ?? "",
    type: c.type ?? "",
    rarity: c.rarity ?? "",
    pool: c.pool ?? "",
    target: c.target ?? "",
    cost: c.cost,
    costUp: c.upgrade ? c.upgrade.cost : null,
  });
  console.log(
    `card\t${c.id}\t${flat(c.name)}\t${flat(resolveMarkup(c.text ?? "", false))}\t` +
      `${c.upgrade ? flat(resolveMarkup(c.text ?? "", true)) : ""}\t${meta}`,
  );
}
for (const r of await read("relics")) {
  const meta = JSON.stringify({ tier: r.tier ?? "", pool: r.pool ?? "" });
  console.log(
    `relic\t${r.id}\t${flat(r.name)}\t${flat(resolveMarkup(r.text ?? "", false))}\t\t${meta}`,
  );
}
for (const p of await read("potions")) {
  const meta = JSON.stringify({ class: p.class ?? "", rarity: p.rarity ?? "" });
  console.log(
    `potion\t${p.id}\t${flat(p.name)}\t${flat(resolveMarkup(p.text ?? "", false))}\t\t${meta}`,
  );
}
