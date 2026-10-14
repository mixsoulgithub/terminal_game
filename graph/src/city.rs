//! 内嵌的城市线路图.加新城市:先用 tools/fetch_city.py 抓数据、tools/osm2dot.py 生成
//! docs/<slug>-subway.dot,然后在这里加一行.

pub struct City {
    pub slug: &'static str,
    pub name: &'static str,
    /// 线路图 .dot 原文
    pub dot: &'static str,
}

pub const CITIES: &[City] = &[
    City {
        slug: "hangzhou",
        name: "杭州",
        dot: include_str!("../docs/hangzhou-subway.dot"),
    },
    City {
        slug: "shanghai",
        name: "上海",
        dot: include_str!("../docs/shanghai-subway.dot"),
    },
    City {
        slug: "chengdu",
        name: "成都",
        dot: include_str!("../docs/chengdu-subway.dot"),
    },
    City {
        slug: "shenzhen",
        name: "深圳",
        dot: include_str!("../docs/shenzhen-subway.dot"),
    },
];

/// 按 slug 找城市,找不到就把可选的列出来
pub fn by_slug(slug: &str) -> Result<&'static City, String> {
    CITIES
        .iter()
        .find(|c| c.slug == slug)
        .ok_or_else(|| format!("没有 {slug} 这个城市(可选:{})", slugs()))
}

pub fn slugs() -> String {
    CITIES
        .iter()
        .map(|c| format!("{}={}", c.slug, c.name))
        .collect::<Vec<_>>()
        .join(" ")
}
