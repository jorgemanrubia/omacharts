import QtQuick

// The omacharts mark, drawn rather than loaded: a bar icon has to take the
// bar's colour, and an image cannot.
Item {
  id: root
  property real iconSize: 14
  // Always bound by the caller: the bar's colour in the bar, the palette's
  // text colour in the panel.
  property color color

  implicitWidth: iconSize
  implicitHeight: iconSize
  width: iconSize
  height: iconSize

  // Three candles, in the proportions of the app's own icon.
  Repeater {
    model: [
      { x: 0.02, wick: [0.18, 0.80], body: [0.32, 0.64] },
      { x: 0.38, wick: [0.30, 0.94], body: [0.42, 0.78] },
      { x: 0.74, wick: [0.06, 0.72], body: [0.18, 0.56] }
    ]
    delegate: Item {
      anchors.fill: parent

      Rectangle {
        x: (modelData.x + 0.09) * root.iconSize
        y: modelData.wick[0] * root.iconSize
        width: Math.max(1, root.iconSize * 0.07)
        height: (modelData.wick[1] - modelData.wick[0]) * root.iconSize
        color: root.color
      }
      Rectangle {
        x: modelData.x * root.iconSize
        y: modelData.body[0] * root.iconSize
        width: root.iconSize * 0.24
        height: (modelData.body[1] - modelData.body[0]) * root.iconSize
        radius: root.iconSize * 0.05
        color: root.color
      }
    }
  }
}
