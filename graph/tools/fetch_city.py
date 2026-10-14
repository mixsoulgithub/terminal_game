"""按城市抓 OSM 地铁数据,给线路图渲染用.

每个城市出两份:
  data/osm/<slug>_routes.json    route=subway 关系(带成员几何),里面是各线路的 stop 顺序和 colour
  data/osm/<slug>_stations.json  station=subway 的命名站点节点(站名 -> 坐标)

线路和站点都是"整个 bbox 一次查"下来(按格查要几百次请求,慢几十倍):
站点用三种常见标法各查一次,哪种整城查不动,才把那种退回 0.06 度分格扫,
分格结果缓存在 .cache/<slug>_<i>_<j>.json,可反复重跑.
线路按 network 标签筛;没命中的 network 会连条数一起打印,方便发现漏掉的运营方.
Overpass 主站经常 504/SSL 掉线,所以备了几个镜像轮着重试.

用法: python3 tools/fetch_city.py --city shanghai [--city chengdu] [--cache .cache] [--refresh-routes]
      python3 tools/fetch_city.py --all
"""
import argparse, collections, concurrent.futures as cf
import json, math, os, sys, time, urllib.parse, urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# Overpass 主站常限流,备几个镜像轮着用
EPS = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
    "https://overpass.osm.jp/api/interpreter",
]
UA = {"User-Agent": "metro-map-fetch-city/1.0 (data preparation)"}
STEP = 0.06                         # 站点分格边长(度)

# slug -> 中文名 / network 标签值列表 / bbox(s, w, n, e)
CITIES = {
    "shanghai": {"name": "上海", "networks": ["上海地铁"],
                 "bbox": (30.55, 120.75, 31.62, 122.05)},
    "chengdu": {"name": "成都", "networks": ["成都地铁"],
                "bbox": (30.20, 103.50, 31.10, 104.60)},
    "shenzhen": {"name": "深圳", "networks": ["深圳地铁"],
                 "bbox": (22.30, 113.65, 22.90, 114.75)},
}


def overpass(query, tries=6, timeout=120):
    """POST 查询;失败(504/SSL/超时)就换镜像 + 退避重试,全挂了才抛."""
    data = urllib.parse.urlencode({"data": query}).encode()
    last = ""
    for attempt in range(tries):
        try:
            ep = EPS[attempt % len(EPS)]
            req = urllib.request.Request(ep, data=data, headers=UA)
            with urllib.request.urlopen(req, timeout=timeout) as r:
                txt = r.read().decode("utf-8", "replace")
            if txt.lstrip().startswith("{"):
                return json.loads(txt)["elements"]
            last = txt[:60]
        except Exception as ex:
            last = "%s @ %s" % (str(ex)[:50], ep.rsplit("/", 2)[-2])
        time.sleep(2.0 + 3.0 * attempt)
    raise RuntimeError(last)


def fetch_routes(bbox):
    """抓 bbox 内全部 route=subway 关系,不带 network 过滤,便于统计漏网网络."""
    q = ('[out:json][timeout:300];'
         'rel["route"="subway"](%f,%f,%f,%f);out geom;' % bbox)
    # 整个 bbox 的 out geom 是大查询,超时给宽一点,重试多几次
    return overpass(q, tries=6, timeout=150)


def split_routes(els, networks):
    """按 network 标签分成命中/未命中两份,未命中的按 network 计数."""
    want = set(networks)
    kept = [e for e in els if (e.get("tags") or {}).get("network") in want]
    others = collections.Counter(
        (e.get("tags") or {}).get("network") for e in els
        if (e.get("tags") or {}).get("network") not in want)
    return kept, others


def cells(routes):
    seen = set()
    for e in routes:
        for m in e.get("members", []):
            if m["type"] == "node" and m.get("role") == "stop":
                seen.add((math.floor(m["lat"] / STEP), math.floor(m["lon"] / STEP)))
            for p in m.get("geometry") or []:
                seen.add((math.floor(p["lat"] / STEP), math.floor(p["lon"] / STEP)))
    return sorted(seen)


STATION_TAGS = [
    'node["station"="subway"]',
    'node["railway"="station"]["subway"="yes"]',
    'node["public_transport"="station"]["subway"="yes"]',
]


def fetch_stations_fast(bbox, path, cache, slug, routes):
    """先整城一次性查站点节点(比按 0.06 度分格快几十倍),
    哪种标法整城查不动,就只把那种标法退回按格扫"""
    s, w, n, e = bbox
    nodes = {}
    fallback = []
    for tag in STATION_TAGS:
        try:
            els = overpass('[out:json][timeout:300];%s(%.4f,%.4f,%.4f,%.4f);out body;'
                           % ((tag, s, w, n, e)), tries=4, timeout=300)
        except Exception as ex:
            print("%s: 整城查 %s 失败(%s),这一种退回按格扫" % (slug, tag, str(ex)[:40]),
                  file=sys.stderr)
            fallback.append(tag)
            continue
        for nd in els:
            if nd.get("type") == "node":
                nodes[nd["id"]] = nd
    ncell = nfail = 0
    if fallback:
        cell_nodes, ncell, nfail = fetch_stations(routes, cache, slug, path, tags=fallback)
        nodes.update(cell_nodes)
    json.dump({"version": 0.6, "elements": list(nodes.values())},
              open(path, "w", encoding="utf-8"), ensure_ascii=False)
    return nodes, ncell, nfail

def fetch_stations(routes, cache, slug, path, tags=None):
    tags = tags or STATION_TAGS

    def one(cell):
        ci, cj = cell
        key = os.path.join(cache, "%s_%d_%d.json" % (slug, ci, cj))
        if os.path.exists(key):
            return
        bbox = (ci * STEP, cj * STEP, (ci + 1) * STEP, (cj + 1) * STEP)
        # 车站的三种常见标法都要: 只标 station=subway 的、railway=station+subway=yes 的、
        # public_transport=station+subway=yes 的
        box = "%.4f,%.4f,%.4f,%.4f" % bbox
        body = "".join(t + "(%s);" % box for t in tags)
        els = overpass('[out:json][timeout:60];(' + body + ');out body;', timeout=60)
        json.dump(els, open(key, "w", encoding="utf-8"), ensure_ascii=False)

    todo = cells(routes)
    fails = []
    with cf.ThreadPoolExecutor(max_workers=3) as ex:
        futs = {ex.submit(one, c): c for c in todo}
        for i, f in enumerate(cf.as_completed(futs), 1):
            try:
                f.result()
            except Exception as ex2:
                fails.append(futs[f])
                print("失败 %s: %s" % (futs[f], ex2), file=sys.stderr)
            if i % 10 == 0:
                print("进度 %d/%d" % (i, len(todo)), file=sys.stderr, flush=True)
    # 首轮失败的格子补抓
    for c in list(fails):
        for _ in range(6):
            try:
                one(c)
                fails.remove(c)
                break
            except Exception:
                time.sleep(5)
    nodes = {}
    for f in os.listdir(cache):
        parts = f[:-5].split("_") if f.endswith(".json") else []
        # 只认本城市的格子文件 <slug>_<i>_<j>.json,别把杭州的 v2_i_j.json 或别的文件算进来
        if len(parts) != 3 or parts[0] != slug or not all(p.isdigit() for p in parts[1:]):
            continue
        for e in json.load(open(os.path.join(cache, f), encoding="utf-8")):
            if e.get("type") == "node":
                nodes[e["id"]] = e
    json.dump({"version": 0.6, "elements": list(nodes.values())},
              open(path, "w", encoding="utf-8"), ensure_ascii=False)
    return nodes, len(todo), len(fails)


def line_refs(routes):
    """各关系的线路编号(ref),空的不要,返回去重后的列表."""
    return sorted({(e.get("tags") or {}).get("ref") for e in routes
                   if (e.get("tags") or {}).get("ref")})


def no_stop(routes):
    """没有 role=stop 的 node 成员的关系,正常不该有."""
    out = []
    for e in routes:
        if not any(m["type"] == "node" and m.get("role") == "stop"
                   for m in e.get("members", [])):
            out.append((e.get("tags") or {}).get("ref") or e["id"])
    return out


def run_city(slug, cache, refresh_routes):
    city = CITIES[slug]
    routes_path = os.path.join(ROOT, "data", "osm", "%s_routes.json" % slug)
    stations_path = os.path.join(ROOT, "data", "osm", "%s_stations.json" % slug)
    if os.path.exists(routes_path) and not refresh_routes:
        routes = json.load(open(routes_path, encoding="utf-8"))["elements"]
        others = None
    else:
        els = fetch_routes(city["bbox"])
        routes, others = split_routes(els, city["networks"])
        json.dump({"version": 0.6, "elements": routes},
                  open(routes_path, "w", encoding="utf-8"), ensure_ascii=False)
    if others is not None:
        print("%s: bbox 内共 %d 条 route=subway 关系, 命中 %d 条"
              % (slug, len(routes) + sum(others.values()), len(routes)), file=sys.stderr)
        if others:
            print("# 未命中的其它 network", file=sys.stderr)
            for net, n in others.most_common():
                print("%s,%s,%d" % (slug, net if net else "(无 network)", n))
        else:
            print("# 未命中的其它 network: 无")
    else:
        print("# network 统计: 走的是缓存(%s),加 --refresh-routes 重抓" % routes_path)

    nodes, ncell, nfail = fetch_stations_fast(city["bbox"], stations_path, cache, slug, routes)
    refs = line_refs(routes)
    bad = no_stop(routes)
    print("slug,名称,关系条数,distinct_ref,站点节点数,格子数,失败格子数,缺stop成员的关系")
    print("%s,%s,%d,%d,%d,%d,%d,%d%s"
          % (slug, city["name"], len(routes), len(refs), len(nodes), ncell, nfail,
             len(bad), ("  " + "|".join(map(str, bad))) if bad else ""))
    print("%s routes %d B, stations %d B"
          % (slug, os.path.getsize(routes_path), os.path.getsize(stations_path)))
    return {"slug": slug, "routes": len(routes), "refs": len(refs),
            "nodes": len(nodes), "cells": ncell, "fails": nfail, "bad": bad}


def print_usage():
    print("用法: python3 tools/fetch_city.py --city <slug> [--city <slug> ...]")
    print("      python3 tools/fetch_city.py --all [--cache .cache] [--refresh-routes]")
    print("# 可选城市")
    print("slug,名称,networks,s,w,n,e")
    for slug, c in CITIES.items():
        print("%s,%s,%s,%s" % (slug, c["name"], "|".join(c["networks"]),
                               ",".join("%g" % v for v in c["bbox"])))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--city", action="append", default=[], help="城市 slug,可重复")
    ap.add_argument("--all", action="store_true", help="抓城市表里的所有城市")
    ap.add_argument("--cache", default=os.path.join(ROOT, ".cache"))
    ap.add_argument("--refresh-routes", action="store_true", help="忽略已抓的线路文件,重抓")
    args = ap.parse_args()
    slugs = list(dict.fromkeys(args.city + (list(CITIES) if args.all else [])))
    if not slugs:
        print_usage()
        return 0
    unknown = [s for s in slugs if s not in CITIES]
    if unknown:
        print("未知城市: %s" % ", ".join(unknown), file=sys.stderr)
        print_usage()
        return 1
    os.makedirs(args.cache, exist_ok=True)
    os.makedirs(os.path.join(ROOT, "data", "osm"), exist_ok=True)
    for slug in slugs:
        run_city(slug, args.cache, args.refresh_routes)
    return 0


if __name__ == "__main__":
    sys.exit(main())
