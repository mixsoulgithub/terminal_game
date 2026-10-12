//! 原版 Slay the Spire 存档(.save / .autosave)导入.
//!
//! 原版格式(从游戏 desktop-1.0.jar 的 SaveAndContinue / SaveFileObfuscator 反编译得到):
//!   一份扁平 JSON(117 个键) -> 与字面量 key 逐字节循环异或 -> 标准 Base64 写盘.
//!   想读回来时如果文本里没有 '{' 就先解码, 所以未混淆的纯 JSON 也能直接吃.
//! 战斗现场不在存档里: 原版读回来是按随机数计数器重放这一层房间, 我们也一样.
//!
//! 这里把原版存档翻成本游戏的存档文本, 之后交给 Run::from_save 正常读档.
//! 认不出的卡/遗物/事件会明确报错, 不会静默丢东西.

use std::path::{Path, PathBuf};

/// 原版混淆用的键(反编译里就是字面量 "key")
const OBF_KEY: &[u8] = b"key";

/// 原版存档目录里认这些后缀
const SAVE_EXT: [&str; 3] = ["autosave", "save", "backUp"];

/// 存档是否被混淆: 原版就是看文本里有没有 '{'
pub fn looks_obfuscated(text: &str) -> bool {
    !text.contains('{')
}

/// 解混淆: Base64 -> 与 key 循环异或 -> UTF-8
pub fn decode(text: &str) -> Result<String, String> {
    if !looks_obfuscated(text) {
        return Ok(text.to_string());
    }
    let raw = base64_decode(text.trim())?;
    let bytes: Vec<u8> = raw
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ OBF_KEY[i % OBF_KEY.len()])
        .collect();
    String::from_utf8(bytes).map_err(|_| "存档解码后不是 UTF-8 文本".to_string())
}

/// 混淆(只给测试用, 让测试能造出一份"原版格式"的存档)
#[cfg(test)]
pub fn encode(text: &str) -> String {
    let bytes: Vec<u8> = text
        .as_bytes()
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ OBF_KEY[i % OBF_KEY.len()])
        .collect();
    base64_encode(&bytes)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[cfg(test)]
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for ch in text.chars() {
        if ch.is_whitespace() || ch == '=' {
            continue;
        }
        let v = B64
            .iter()
            .position(|c| *c as char == ch)
            .ok_or_else(|| format!("存档里有非 Base64 字符 {ch:?}"))?;
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- 极简 JSON
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_i64(&self, dflt: i64) -> i64 {
        match self {
            Json::Num(n) => *n as i64,
            Json::Str(s) => s.trim().parse().unwrap_or(dflt),
            _ => dflt,
        }
    }

    fn as_bool(&self, dflt: bool) -> bool {
        match self {
            Json::Bool(b) => *b,
            _ => dflt,
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            _ => "",
        }
    }

    fn as_arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => &[],
        }
    }

    fn as_strs(&self) -> Vec<String> {
        self.as_arr()
            .iter()
            .filter_map(|v| match v {
                Json::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> u8 {
        *self.bytes.get(self.pos).unwrap_or(&0)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), b' ' | b'\t' | b'\r' | b'\n') {
            self.pos += 1;
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        self.skip_ws();
        if self.peek() != b {
            return Err(format!("存档 JSON 第 {} 字节处期望 {:?}", self.pos, b as char));
        }
        self.pos += 1;
        Ok(())
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            b'{' => {
                self.pos += 1;
                let mut kv = Vec::new();
                self.skip_ws();
                if self.peek() == b'}' {
                    self.pos += 1;
                    return Ok(Json::Obj(kv));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.expect(b':')?;
                    let v = self.value()?;
                    kv.push((key, v));
                    self.skip_ws();
                    match self.peek() {
                        b',' => self.pos += 1,
                        b'}' => {
                            self.pos += 1;
                            return Ok(Json::Obj(kv));
                        }
                        c => return Err(format!("存档 JSON 第 {} 字节处多了 {:?}", self.pos, c as char)),
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                let mut out = Vec::new();
                self.skip_ws();
                if self.peek() == b']' {
                    self.pos += 1;
                    return Ok(Json::Arr(out));
                }
                loop {
                    out.push(self.value()?);
                    self.skip_ws();
                    match self.peek() {
                        b',' => self.pos += 1,
                        b']' => {
                            self.pos += 1;
                            return Ok(Json::Arr(out));
                        }
                        c => return Err(format!("存档 JSON 第 {} 字节处多了 {:?}", self.pos, c as char)),
                    }
                }
            }
            b'"' => Ok(Json::Str(self.string()?)),
            b't' | b'f' => {
                let rest = &self.bytes[self.pos..];
                if rest.starts_with(b"true") {
                    self.pos += 4;
                    Ok(Json::Bool(true))
                } else if rest.starts_with(b"false") {
                    self.pos += 5;
                    Ok(Json::Bool(false))
                } else {
                    Err(format!("存档 JSON 第 {} 字节处不认识的常量", self.pos))
                }
            }
            b'n' => {
                if self.bytes[self.pos..].starts_with(b"null") {
                    self.pos += 4;
                    Ok(Json::Null)
                } else {
                    Err(format!("存档 JSON 第 {} 字节处不认识的常量", self.pos))
                }
            }
            _ => {
                let start = self.pos;
                while matches!(self.peek(), b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                    self.pos += 1;
                }
                let s = std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| "存档 JSON 里的数字不是 ASCII".to_string())?;
                s.parse::<f64>()
                    .map(Json::Num)
                    .map_err(|_| format!("存档 JSON 第 {} 字节处的数字 {s:?} 不认识", start))
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.peek() != b'"' {
            return Err(format!("存档 JSON 第 {} 字节处期望字符串", self.pos));
        }
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let c = self.bytes[self.pos];
            self.pos += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.bytes.get(self.pos).copied().unwrap_or(0);
                    self.pos += 1;
                    match e {
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let hex = std::str::from_utf8(
                                self.bytes.get(self.pos..self.pos + 4).unwrap_or(b"0000"),
                            )
                            .unwrap_or("0000");
                            self.pos += 4;
                            let n = u32::from_str_radix(hex, 16).unwrap_or(0);
                            out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                        }
                        other => out.push(other as char),
                    }
                }
                other => {
                    // 原版是 UTF-8 文本, 多字节字符按原样收进来
                    let start = self.pos - 1;
                    let len = if other < 0x80 {
                        1
                    } else if other >> 5 == 0b110 {
                        2
                    } else if other >> 4 == 0b1110 {
                        3
                    } else {
                        4
                    };
                    let end = (start + len).min(self.bytes.len());
                    self.pos = end;
                    if let Ok(s) = std::str::from_utf8(&self.bytes[start..end]) {
                        out.push_str(s);
                    }
                }
            }
        }
        Err("存档 JSON 里的字符串没有收尾引号".to_string())
    }
}

fn parse_json(text: &str) -> Result<Json, String> {
    let mut p = Parser::new(text);
    let v = p.value()?;
    Ok(v)
}

// ---------------------------------------------------------------- id 归一
/// 只留小写字母数字, 用来跨命名习惯对 id(StS 用 "Burning Blood", 我们用 burning_blood)
fn norm_id(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// 原版卡牌 id 转本游戏卡牌 id: 先归一匹配, 再退一步去掉 _R/_G/_B/_P 这类职业后缀
fn resolve_card(id: &str) -> Option<&'static str> {
    let want = norm_id(id);
    if let Some(d) = crate::core::cards::CARDS.iter().find(|d| norm_id(d.id) == want) {
        return Some(d.id);
    }
    if let Some(d) = crate::core::events::EVENT_CARDS
        .iter()
        .find(|d| norm_id(d.id) == want)
    {
        return Some(d.id);
    }
    // Strike_R / Defend_G / Strike_B / Strike_P: 去掉最后一个下划线段再试
    if let Some((head, _)) = id.rsplit_once('_') {
        let want = norm_id(head);
        if let Some(d) = crate::core::cards::CARDS.iter().find(|d| norm_id(d.id) == want) {
            return Some(d.id);
        }
    }
    None
}

fn resolve_relic(id: &str) -> Option<&'static str> {
    let want = norm_id(id);
    if let Some(r) = crate::core::relics::RELICS.iter().find(|r| norm_id(r.id) == want) {
        return Some(r.id);
    }
    crate::core::events::EVENT_RELICS
        .iter()
        .copied()
        .find(|r| norm_id(r) == want)
}

fn resolve_potion(id: &str) -> Option<&'static str> {
    let want = norm_id(id);
    crate::core::potions::POTIONS
        .iter()
        .find(|p| norm_id(p.id) == want)
        .map(|p| p.id)
}

fn resolve_enemy(id: &str) -> Option<&'static str> {
    let want = norm_id(id);
    crate::core::enemies::ENEMIES
        .iter()
        .find(|e| norm_id(e.id) == want)
        .map(|e| e.id)
}

fn resolve_event(id: &str) -> Option<&'static str> {
    let want = norm_id(id);
    crate::core::events::EVENTS
        .iter()
        .find(|e| norm_id(e.id) == want)
        .map(|e| e.id)
}

// ---------------------------------------------------------------- 导入
/// 原版存档文本 -> (本游戏存档文本, 提示). 提示里放"降级处理"的地方, 界面上要显示
pub fn import(text: &str, file_name: &str) -> Result<(String, Vec<String>), String> {
    let json = decode(text)?;
    let root = parse_json(&json).map_err(|e| format!("这份文件不是原版存档: {e}"))?;
    if !matches!(root, Json::Obj(_)) {
        return Err("这份文件不是原版存档: 顶层不是 JSON 对象".to_string());
    }
    let mut warn: Vec<String> = Vec::new();
    if root.get("is_endless_mode").is_some_and(|v| v.as_bool(false)) {
        return Err("这是无尽模式存档, 本作还没有无尽模式".to_string());
    }
    if root.get("is_daily").is_some_and(|v| v.as_bool(false))
        || root.get("is_trial").is_some_and(|v| v.as_bool(false))
        || !root.get("custom_mods").map(|v| v.as_strs()).unwrap_or_default().is_empty()
    {
        return Err("这是每日/自定义模式存档, 本作不支持".to_string());
    }
    // 角色: 原版存档不存职业, 用玩家名或文件名认
    let char_id = resolve_char(root.get("name").map(|v| v.as_str()).unwrap_or(""), file_name)
        .ok_or_else(|| {
            format!(
                "认不出存档属于哪个角色(玩家名 {:?}, 文件 {file_name}); 只支持铁甲战士/静默猎手",
                root.get("name").map(|v| v.as_str()).unwrap_or("")
            )
        })?;
    let seed = root.get("seed").map(|v| v.as_i64(0)).unwrap_or(0);
    let asc = if root.get("is_ascension_mode").is_some_and(|v| v.as_bool(false)) {
        root.get("ascension_level").map(|v| v.as_i64(0)).unwrap_or(0)
    } else {
        0
    };
    let hp = root.get("current_health").map(|v| v.as_i64(1)).unwrap_or(1).max(1);
    let max_hp = root
        .get("max_health")
        .map(|v| v.as_i64(hp))
        .unwrap_or(hp)
        .max(hp);
    let gold = root.get("gold").map(|v| v.as_i64(0)).unwrap_or(0).max(0);
    let act = root.get("act_num").map(|v| v.as_i64(1)).unwrap_or(1).clamp(1, 4);
    let floor = root.get("floor_num").map(|v| v.as_i64(0)).unwrap_or(0).max(0);

    let mut out = String::new();
    out.push_str(&format!("seed={seed}\n"));
    out.push_str(&format!("char={char_id}\n"));
    out.push_str(&format!("ascension={asc}\n"));
    out.push_str(&format!("hp={hp}\n"));
    out.push_str(&format!("max_hp={max_hp}\n"));
    out.push_str(&format!("gold={gold}\n"));
    out.push_str(&format!("act={act}\n"));
    out.push_str(&format!("floor_num={floor}\n"));
    out.push_str(&format!("floor={floor}\n"));
    // 三把钥匙
    let key = |n: &str| if root.get(n).is_some_and(|v| v.as_bool(false)) { '1' } else { '0' };
    out.push_str(&format!(
        "keys={}{}{}\n",
        key("has_emerald_key"),
        key("has_ruby_key"),
        key("has_sapphire_key")
    ));
    // 随机数计数器: 原版只存计数器, 我们按名字收下来, 读档时在"按种子新建的流"上设回去
    // (见 Run::from_save 的 rng_counters). 原版没存的流(杂项/怪物血量/Neow)按 0 起,
    // 与原版重开一条流的做法一致
    let mut counters_line = Vec::new();
    for (ours, theirs) in [
        ("cardRng", "card_seed_count"),
        ("eventRng", "event_seed_count"),
        ("merchantRng", "merchant_seed_count"),
        ("monsterRng", "monster_seed_count"),
        ("potionRng", "potion_seed_count"),
        ("relicRng", "relic_seed_count"),
        ("treasureRng", "treasure_seed_count"),
        ("aiRng", "ai_seed_count"),
        ("cardRandomRng", "card_random_seed_count"),
        ("shuffleRng", "shuffle_seed_count"),
    ] {
        let n = root.get(theirs).map(|v| v.as_i64(0)).unwrap_or(0).max(0);
        counters_line.push(format!("{ours}:{n}"));
    }
    out.push_str(&format!("rng_counters={}\n", counters_line.join(",")));
    if let Some(boss) = root.get("boss").map(|v| v.as_str()).filter(|s| !s.is_empty()) {
        match resolve_enemy(boss) {
            Some(id) => out.push_str(&format!("boss={id}\n")),
            None => warn.push(format!("Boss {boss} 本作还没实现, 按本幕默认 Boss 起")),
        }
    }
    let potion_chance = root.get("potion_chance").map(|v| v.as_i64(0)).unwrap_or(0);
    out.push_str(&format!("potion_pity={}\n", potion_chance.clamp(0, 100)));
    out.push_str(&format!("card_factor={}\n", root.get("card_random_seed_randomizer").map(|v| v.as_i64(0)).unwrap_or(0).max(0)));
    // ? 房三种概率
    if let Some(chances) = root.get("event_chances") {
        let v: Vec<i32> = chances
            .as_arr()
            .iter()
            .map(|x| x.as_i64(0) as i32)
            .collect();
        if v.len() == 3 {
            out.push_str(&format!("blizzard={},{},{}\n", v[0], v[1], v[2]));
        }
    }
    // 商店删牌次数: 原版存的是当前价格(75 起, 每删一次 +25)
    let purge = root.get("purgeCost").map(|v| v.as_i64(75)).unwrap_or(75);
    out.push_str(&format!("removes={}\n", ((purge.max(75) - 75) / 25).max(0)));
    // 战斗池: 原版存的是"还没抽到的怪物名", 我们这边的池子是遭遇组(怪的组合),
    // 两者不是一回事, 不硬套; 本作按种子重洗本章池子
    if root.get("monster_list").is_some() {
        warn.push("战斗遭遇池按本作规则重洗(原版存的是怪物名单,不是遭遇组)".to_string());
    }
    // 事件池: 原版存事件名, 我们存事件 id, 名字对得上就照抄
    for (ours, theirs) in [("events", "event_list"), ("one_time", "one_time_event_list")] {
        let ids: Vec<String> = root.get(theirs).map(|v| v.as_strs()).unwrap_or_default();
        if ids.is_empty() {
            continue;
        }
        let mut mapped = Vec::new();
        let mut bad = Vec::new();
        for id in &ids {
            match resolve_event(id) {
                Some(x) => mapped.push(x),
                None => bad.push(id.clone()),
            }
        }
        if bad.is_empty() {
            out.push_str(&format!("{ours}={}\n", mapped.join(",")));
        } else {
            warn.push(format!(
                "{ours} 池里有本作没实现的: {} (这一项按种子重洗)",
                bad.join(" ")
            ));
        }
    }
    // 牌组
    let mut deck = Vec::new();
    for c in root.get("cards").map(|v| v.as_arr()).unwrap_or(&[]) {
        let raw = c.get("id").map(|v| v.as_str()).unwrap_or("");
        let id = resolve_card(raw)
            .ok_or_else(|| format!("存档里的卡 {raw} 本作还没实现, 无法导入"))?;
        let plus = c.get("upgrades").map(|v| v.as_i64(0)).unwrap_or(0).max(0);
        // 原版的 misc 是牌自身的累积值(仪式匕首的成长等), 对应我们的 bonus
        let misc = c.get("misc").map(|v| v.as_i64(0)).unwrap_or(0).max(0);
        deck.push(format!(
            "{id}:{}:{plus}:{misc}:0",
            if plus > 0 { 1 } else { 0 }
        ));
    }
    if deck.is_empty() {
        return Err("存档里的牌组是空的, 不像是能导入的存档".to_string());
    }
    out.push_str(&format!("deck={}\n", deck.join(",")));
    // 遗物(带原版计数器: 钢笔尖/幸福花/香炉/日晷这四件跨战斗计数)
    let relics: Vec<String> = root.get("relics").map(|v| v.as_strs()).unwrap_or_default();
    let counters: Vec<i64> = root
        .get("relic_counters")
        .map(|v| v.as_arr().iter().map(|x| x.as_i64(0)).collect())
        .unwrap_or_default();
    let mut relic_ids = Vec::new();
    let mut bad_relics = Vec::new();
    let mut rc = [0i32; 6];
    for (i, r) in relics.iter().enumerate() {
        match resolve_relic(r) {
            Some(id) => {
                relic_ids.push(id);
                let n = counters.get(i).copied().unwrap_or(0).max(0) as i32;
                match id {
                    "pen_nib" => rc[0] = n,
                    "happy_flower" => rc[1] = n,
                    "incense_burner" => rc[2] = n,
                    "sundial" => rc[3] = n,
                    _ => {}
                }
            }
            None => bad_relics.push(r.clone()),
        }
    }
    if !bad_relics.is_empty() {
        return Err(format!(
            "存档里的遗物本作还没实现: {}, 无法导入",
            bad_relics.join(" ")
        ));
    }
    out.push_str(&format!("relics={}\n", relic_ids.join(",")));
    out.push_str(&format!(
        "relic_counters={},{},{},{},{},{},0\n",
        rc[0], rc[1], rc[2], rc[3], rc[4], rc[5]
    ));
    // 药水
    let mut potions = Vec::new();
    let mut bad_potions = Vec::new();
    for p in root.get("potions").map(|v| v.as_strs()).unwrap_or_default() {
        match resolve_potion(&p) {
            Some(id) => potions.push(id),
            None => bad_potions.push(p),
        }
    }
    if !bad_potions.is_empty() {
        return Err(format!(
            "存档里的药水本作还没实现: {}, 无法导入",
            bad_potions.join(" ")
        ));
    }
    out.push_str(&format!("potions={}\n", potions.join(",")));
    // 我们这边的 potion_slots 记的是"当前有几瓶"(容量由遗物重算), 不是原版的槽位容量
    out.push_str(&format!("potion_slots={}\n", potions.len()));
    // 地图路径: 原版存的是坐标, 我们按种子重建地图之后翻译成节点下标(见 Run::from_save)
    let xs: Vec<i64> = root.get("path_x").map(|v| v.as_arr().iter().map(|x| x.as_i64(0)).collect()).unwrap_or_default();
    let ys: Vec<i64> = root.get("path_y").map(|v| v.as_arr().iter().map(|x| x.as_i64(0)).collect()).unwrap_or_default();
    let mut xy: Vec<String> = xs
        .iter()
        .zip(ys.iter())
        .map(|(x, y)| format!("{x}:{y}"))
        .collect();
    // 当前房间单独一行, 免得原版没把它记进 path 时位置对不上
    let room = (
        root.get("room_x").map(|v| v.as_i64(-1)).unwrap_or(-1),
        root.get("room_y").map(|v| v.as_i64(-1)).unwrap_or(-1),
    );
    if room.0 >= 0 && room.1 >= 0 {
        let cur = format!("{}:{}", room.0, room.1);
        if xy.last() != Some(&cur) {
            xy.push(cur);
        }
    }
    if !xy.is_empty() {
        out.push_str(&format!("path_xy={}\n", xy.join(",")));
    }
    warn.push("导入的存档恢复到地图上, 需要重新进入当前房间".to_string());
    Ok((out, warn))
}

/// 从文件名/玩家名认角色(原版存档不写职业, 靠 AUTOSAVE 文件名)
fn resolve_char(player_name: &str, file_name: &str) -> Option<&'static str> {
    let candidates = [norm_id(player_name), norm_id(file_name)];
    crate::core::roster::all()
        .iter()
        .find(|c| candidates.iter().any(|n| n.contains(&norm_id(c.id)) || norm_id(c.name) == *n))
        .map(|c| c.id)
}

/// 读一份原版存档文件
pub fn import_file(path: &Path) -> Result<(String, Vec<String>), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读不了 {}: {e}", path.display()))?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    import(&text, &name)
}

/// 原版存档的常见位置(Steam 库 + 本机 steamcmd 装的那份)
pub fn default_paths() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let home = std::env::var("HOME").unwrap_or_default();
    let mut steam_roots: Vec<PathBuf> = vec![
        PathBuf::from(&home).join(".steam/steam"),
        PathBuf::from(&home).join(".local/share/Steam"),
        PathBuf::from(&home).join("Steam"),
        PathBuf::from(&home).join(".steam/root"),
    ];
    for root in steam_roots.clone() {
        let vdf = root.join("steamapps/libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            for cap in text.split("\"path\"").skip(1) {
                if let Some(rest) = cap.trim_start().strip_prefix('"') {
                    if let Some((p, _)) = rest.split_once('"') {
                        steam_roots.push(PathBuf::from(p));
                    }
                }
            }
        }
    }
    for root in steam_roots {
        dirs.push(root.join("steamapps/common/Slay the Spire/saves"));
        dirs.push(root.join("steamapps/compatdata/646570/pfx/drive_c/users/steamuser/AppData/Local/SlayTheSpire/saves"));
    }
    dirs.push(PathBuf::from(&home).join(".local/share/SlayTheSpire/saves"));
    dirs.push(PathBuf::from(&home).join("projects/sts-golden/game/saves"));
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let ext = p.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if SAVE_EXT.contains(&ext.as_str()) {
                out.push(p);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真值来自游戏自带的 JRE 跑原版 SaveFileObfuscator.encode(...):
    ///   jjs -cp desktop-1.0.jar 里 SaveFileObfuscator.encode("{\"gold\":99,\"name\":\"Rusty\"}", "key")
    #[test]
    fn obfuscation_matches_the_real_game_class() {
        let plain = "{\"gold\":99,\"name\":\"Rusty\"}";
        let real = "EEceBAkdSV9AUklbBQQUDkdDSTcMGBEASRg=";
        assert!(looks_obfuscated(real));
        assert!(!looks_obfuscated(plain));
        assert_eq!(decode(real).unwrap(), plain);
        assert_eq!(encode(plain), real);
        assert_eq!(decode(plain).unwrap(), plain);
    }

    #[test]
    fn base64_junk_is_rejected() {
        assert!(decode("!!!not base64!!!").is_err());
    }

    /// 造一份和原版同字段名的存档(数值都是真格式), 走完整导入链.
    /// 地图坐标从本作给这个种子生成的地图里取, 这样才是在测坐标翻译本身.
    fn sample_save() -> (String, i64) {
        let probe = crate::core::run::Run::new(3141592653589793);
        let target = probe
            .map
            .nodes
            .iter()
            .position(|n| n.floor == 3)
            .expect("地图第 3 层有节点");
        // 从目标节点往回走出一条三段路径
        let mut chain = vec![target];
        while chain.len() < 3 {
            let prev = probe.map.nodes[*chain.last().unwrap()].prev.first().copied();
            match prev {
                Some(i) => chain.push(i),
                None => break,
            }
        }
        chain.reverse();
        let coords: Vec<String> = chain
            .iter()
            .map(|i| format!("{}:{}", probe.map.nodes[*i].col, probe.map.nodes[*i].floor))
            .collect();
        let room = coords.last().unwrap().clone();
        let floor_num = probe.map.nodes[target].floor + 1;
        let json = format!(
            r#"{{
            "gold": 137,
            "current_health": 61,
            "max_health": 80,
            "ascension_level": 5,
            "is_ascension_mode": true,
            "floor_num": {floor_num},
            "act_num": 1,
            "seed": 3141592653589793,
            "name": "Ironclad",
            "boss": "Hexaghost",
            "has_emerald_key": true,
            "has_ruby_key": false,
            "has_sapphire_key": false,
            "potion_chance": 40,
            "purgeCost": 100,
            "card_random_seed_randomizer": 3,
            "event_chances": [10, 3, 2],
            "monster_seed_count": 11,
            "event_seed_count": 12,
            "merchant_seed_count": 13,
            "card_seed_count": 14,
            "treasure_seed_count": 15,
            "relic_seed_count": 16,
            "potion_seed_count": 17,
            "ai_seed_count": 18,
            "shuffle_seed_count": 19,
            "card_random_seed_count": 20,
            "monster_list": ["Cultist", "Jaw Worm"],
            "elite_monster_list": ["Gremlin Nob"],
            "cards": [
                {{"id": "Strike_R", "upgrades": 0, "misc": 0}},
                {{"id": "Strike_R", "upgrades": 0, "misc": 0}},
                {{"id": "Defend_R", "upgrades": 1, "misc": 0}},
                {{"id": "Bash", "upgrades": 0, "misc": 0}},
                {{"id": "AscendersBane", "upgrades": 0, "misc": 0}}
            ],
            "relics": ["Burning Blood", "Pen Nib"],
            "relic_counters": [0, 4],
            "potions": ["Fire Potion", "Block Potion"],
            "path_x": [{xs}],
            "path_y": [{ys}],
            "room_x": {rx},
            "room_y": {ry},
            "current_room": "Room - Monster"
        }}"#,
            xs = chain.iter().map(|i| probe.map.nodes[*i].col.to_string()).collect::<Vec<_>>().join(", "),
            ys = chain.iter().map(|i| probe.map.nodes[*i].floor.to_string()).collect::<Vec<_>>().join(", "),
            rx = probe.map.nodes[target].col,
            ry = probe.map.nodes[target].floor,
        );
        let _ = room;
        (encode(&collapse(&json)), floor_num as i64)
    }

    /// 把多行 JSON 压成一行
    fn collapse(s: &str) -> String {
        s.lines().map(|l| l.trim()).collect::<Vec<_>>().join("")
    }

    /// 导入-读档一条龙: 走的是游戏里真正的 Run::from_save
    #[test]
    fn sample_save_lands_in_our_loader() {
        let (encoded, floor_num) = sample_save();
        let (text, warn) = import(&encoded, "IRONCLAD.autosave").expect("应该能导入");
        assert!(text.contains("char=ironclad"), "{text}");
        assert!(text.contains("ascension=5"), "{text}");
        assert!(text.contains("hp=61"), "{text}");
        assert!(text.contains("gold=137"), "{text}");
        assert!(text.contains("act=1"), "{text}");
        assert!(text.contains("keys=100"), "{text}");
        assert!(text.contains("monsterRng:11"), "{text}");
        assert!(text.contains("shuffleRng:19"), "{text}");
        assert!(text.contains("removes=1"), "删牌次数该从价格反推: {text}");
        let run = crate::core::run::Run::from_save(&text).expect("导入出来的存档要能被读档");
        assert_eq!(run.seed, 3141592653589793);
        assert_eq!(run.player.gold, 137);
        assert_eq!(run.player.hp, 61);
        assert_eq!(run.player.max_hp, 80);
        assert_eq!(run.ascension, 5);
        assert_eq!(run.act, 1);
        assert!(run.keys.emerald);
        assert_eq!(run.player.deck.len(), 5);
        assert_eq!(run.player.deck[0].def.id, "strike");
        assert!(run.player.deck[2].upgraded);
        assert!(run.player.relics.iter().any(|r| r.id == "burning_blood"));
        assert_eq!(run.player.potions.len(), 2);
        assert_eq!(run.path.len(), 3, "地图路径要落到节点下标上: {:?}", run.path);
        // HUD 上的层数是 run.floor() + 1, 要和存档里的 floor_num 对得上
        assert_eq!(
            run.floor() as i64 + 1,
            floor_num,
            "HUD 层数要和存档的 floor_num 一致(节点下标 {:?})",
            run.path.last()
        );
        assert!(!warn.is_empty());
    }

    /// 打击/防御是四个职业共用同一份 CardDef, 静默猎手的要打上绿职记号
    #[test]
    fn imported_basics_get_the_right_colour() {
        let json = r#"{"seed":7,"name":"Silent","current_health":70,"max_health":70,
            "cards":[{"id":"Strike_G","upgrades":0,"misc":0},{"id":"Defend_G","upgrades":0,"misc":0},
                     {"id":"Neutralize","upgrades":0,"misc":0}],
            "relics":["Ring of the Snake"],"potions":[]}"#;
        let (text, _) = import(json, "SILENT.autosave").expect("应该能导入");
        let run = crate::core::run::Run::from_save(&text).expect("要能被读档");
        assert_eq!(run.character, "silent");
        assert_eq!(run.player.deck[0].def.id, "strike");
        assert_eq!(run.player.deck[0].color_hint, Some("green"));
        assert_eq!(run.player.deck[2].def.id, "neutralize");
        assert!(run.player.relics.iter().any(|r| r.id == "ring_of_the_snake"));
    }

    #[test]
    fn unknown_card_is_reported() {
        let json = r#"{"seed":1,"name":"Ironclad","current_health":1,"max_health":1,
            "cards":[{"id":"Not A Real Card","upgrades":0,"misc":0}],"relics":[],"potions":[]}"#;
        let err = import(json, "IRONCLAD.autosave").unwrap_err();
        assert!(err.contains("Not A Real Card"), "{err}");
    }

    #[test]
    fn daily_and_endless_are_refused() {
        let endless = r#"{"seed":1,"name":"Ironclad","is_endless_mode":true,"cards":[],"relics":[],"potions":[]}"#;
        assert!(import(endless, "IRONCLAD.autosave").unwrap_err().contains("无尽"));
        let daily = r#"{"seed":1,"name":"Ironclad","is_daily":true,"cards":[],"relics":[],"potions":[]}"#;
        assert!(import(daily, "IRONCLAD.autosave").unwrap_err().contains("每日"));
    }

    #[test]
    fn garbage_is_refused() {
        assert!(import("hello world", "IRONCLAD.autosave").is_err());
    }

    #[test]
    fn char_can_come_from_the_file_name() {
        assert_eq!(resolve_char("", "SILENT.autosave"), Some("silent"));
        assert_eq!(resolve_char("Ironclad", ""), Some("ironclad"));
        assert_eq!(resolve_char("", "nope.autosave"), None);
    }
}

