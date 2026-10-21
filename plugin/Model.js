// Shaping the CLI's output for the panel.
//
// Kept out of the QML so it can be reasoned about on its own: the panel is a
// list of rows, and working out what a row says is not a layout problem.

.pragma library

function parse(text) {
  if (!text || !text.trim()) return { sections: [], error: "" }
  try {
    var parsed = JSON.parse(text)
    return {
      sections: Array.isArray(parsed.sections) ? parsed.sections : [],
      // The app's palette, derived from the desktop theme and the bar scheme
      // chosen in the window, and sent with the data, so the bar and the
      // window cannot disagree about what up looks like.
      colors: parsed.colors || null,
      updatedAt: parsed.updatedAt || 0,
      error: parsed.error || ""
    }
  } catch (e) {
    return { sections: [], error: "unreadable output" }
  }
}

// Flatten sections into rows the list can render directly: headers and
// entries in one stream, so the list does not have to nest.
function rows(sections) {
  var out = []
  for (var s = 0; s < sections.length; s++) {
    var section = sections[s]
    var entries = Array.isArray(section.entries) ? section.entries : []
    if (!entries.length) continue
    if (!section.root && section.name) {
      out.push({ header: true, name: section.name })
    }
    for (var e = 0; e < entries.length; e++) {
      var entry = entries[e]
      out.push({
        header: false,
        symbol: entry.symbol,
        suffix: entry.suffix || "",
        display: entry.display,
        name: entry.name,
        last: entry.last,
        change: entry.change,
        changePct: entry.changePct,
        kind: entry.kind || "",
        hasQuote: entry.last !== null && entry.last !== undefined,
        spark: Array.isArray(entry.spark) ? entry.spark : []
      })
    }
  }
  return out
}

// Which rows the keyboard may land on. Headers are scenery.
function selectableIndexes(rows) {
  var out = []
  for (var i = 0; i < rows.length; i++) if (!rows[i].header) out.push(i)
  return out
}

function quoteCount(rows) {
  var n = 0
  for (var i = 0; i < rows.length; i++) if (!rows[i].header && rows[i].hasQuote) n++
  return n
}

// The one quote the bar itself shows: the first entry that actually has one.
function headline(sections) {
  var all = rows(sections)
  for (var i = 0; i < all.length; i++) {
    if (!all[i].header && all[i].hasQuote) return all[i]
  }
  return null
}

// Prices of different sizes want different precision. A currency pair moving
// 0.0008 shown to two decimals reads as "nothing happened".
//
// Mirrors the app's own price_decimals, kind and all, so a symbol does not
// read one way in the bar and another in the window. What it cannot mirror is
// the gridline term: the bar has no grid.
function decimals(price, kind) {
  var size = Math.abs(price)
  if (kind === "FX") return size >= 20 ? 3 : 5
  if (size >= 1) return 2
  if (size >= 0.01) return 4
  return 6
}

function formatPrice(value, kind) {
  if (value === null || value === undefined) return "–"
  return Number(value).toFixed(decimals(value, kind))
}

function formatPercent(value) {
  if (value === null || value === undefined) return ""
  var sign = value > 0 ? "+" : ""
  return sign + Number(value).toFixed(2) + "%"
}

function formatChange(value, price, kind) {
  if (value === null || value === undefined) return ""
  var sign = value > 0 ? "+" : ""
  return sign + Number(value).toFixed(decimals(price, kind))
}

// The colours the panel draws in, by role.
//
// Every one comes from the app's palette as it arrived. Nothing is derived
// here: the app has already checked each as text against the panel and a
// hovered row, and a second derivation in the widget would be a second
// palette to drift. Until the first answer arrives there are no rows, and the
// panel's own text and ground stand in, so not one colour in the widget is
// its own.
//
// The change pill is a tag, the chart's last-price tag: a fill in the
// direction's colour and a label the app picked to read on it. Both arrive as
// a pair, so the bar's pill and the chart's tag are the same two colours.
function palette(colors, text, ground) {
  var given = colors || {}
  var pick = function (key, fallback) { return given[key] ? given[key] : fallback }
  var written = pick("text", text)
  var given_tags = given.tags || {}
  var tag = function (way, fill) {
    var t = given_tags[way] || {}
    return { fill: t.fill ? t.fill : fill, label: t.label ? t.label : ground }
  }
  var out = {
    text: written,
    muted: pick("muted", written),
    up: pick("up", written),
    down: pick("down", written),
    flat: pick("flat", pick("muted", written)),
    hover: pick("hover", ground)
  }
  out.tags = { up: tag("up", out.up), down: tag("down", out.down), flat: tag("flat", out.flat) }
  return out
}

// Up, down, or neither. The panel colours from this rather than comparing
// numbers in three places.
function direction(value) {
  if (value === null || value === undefined) return "flat"
  if (value > 0) return "up"
  if (value < 0) return "down"
  return "flat"
}
