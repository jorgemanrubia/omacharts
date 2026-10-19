import QtQuick

// The row's shape, the way the Stocks app draws it: no axes, no scale, just
// where it has been. Normalised to its own range, because the question is the
// shape and not the level — the price beside it already says the level.
Canvas {
  id: root

  property var points: []
  // Always bound by the row, to the palette's colour for the move.
  property color stroke

  antialiasing: true

  onPointsChanged: requestPaint()
  onStrokeChanged: requestPaint()
  onWidthChanged: requestPaint()
  onHeightChanged: requestPaint()

  onPaint: {
    var ctx = getContext("2d")
    ctx.reset()
    if (!points || points.length < 2) return

    var low = points[0]
    var high = points[0]
    for (var i = 1; i < points.length; i++) {
      if (points[i] < low) low = points[i]
      if (points[i] > high) high = points[i]
    }
    // A flat series would divide by zero; draw it down the middle.
    var span = high - low
    if (span <= 0) span = 1

    var inset = 1.5
    var usableWidth = Math.max(1, width - inset * 2)
    var usableHeight = Math.max(1, height - inset * 2)

    ctx.beginPath()
    for (var p = 0; p < points.length; p++) {
      var x = inset + (p / (points.length - 1)) * usableWidth
      var y = inset + (1 - (points[p] - low) / span) * usableHeight
      if (p === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    }
    ctx.strokeStyle = stroke
    ctx.lineWidth = 1.2
    ctx.lineJoin = "round"
    ctx.lineCap = "round"
    ctx.stroke()
  }
}
