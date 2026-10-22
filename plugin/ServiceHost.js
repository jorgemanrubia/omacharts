// Omarchy mounts one bar per monitor but keeps one service per plugin. This
// finds it; the binding that re-runs when the host has built it lives in the
// panel.
function hostedService(bar) {
  var shell = bar ? bar.shell : null
  if (!shell || typeof shell.serviceFor !== "function") return null
  return shell.serviceFor("omacharts.watchlist") || null
}

if (typeof module !== "undefined") {
  module.exports = { hostedService: hostedService }
}
