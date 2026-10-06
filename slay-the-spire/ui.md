# UI

start window (full screen, no top/relic/status bar)
  title: spire art on top, menu box below (continue / new game / compendium / quit), seed line at the bottom of the box
  character select: one centered box, one line per character (name, hp, starting relic, deck size)
  compendium: one centered box, one line per book (card library / relic collection / potion lab)
  shop: list on the left (relic = name colored by rarity, potion = (name)),
        price on the right with a [sold out] / [can't afford] tag in front of the $,
        the whole line goes gray in those two cases,
        detail on the right centered (name / [rarity] / description)
        pressing enter on a gray line shakes the whole line (name, tag, $) for ~6 frames,
1 column each way

  library: full-screen box
    row 1: tab row (one tab per group, colored background)
    row 2: separator joining both borders, with a T joint above the vertical line
    below: list on the left, detail on the right, one vertical line between
    the current tab's count sits at the right end of the tab row
    list row: (cost) + name on the left, target tag right aligned, (not implemented) centered
    detail: same as the upgrade window (cost left, name/type/text centered),
            base on top / upgraded below, separator joins the borders

top bar: blood / blood limitation / block ...
relic bar
battle area
info bar
  battle: energy counter on the left, "in hand n/undrawn n/exhausted n/discard n" on the right
command bar

battle area
  card area | enemy area
  enemy: no box, only the chosen one gets four lit corners

card area
  energy
  card list (centered, no brackets around the cost)
  description
  buff 
  conclude info
