import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model
import "ServiceHost.js" as ServiceHost

// The watchlist, in the bar.
//
// Modelled on the Stocks app everyone already knows: one row per symbol, the
// ticker and its name on the left, the price and a coloured change pill on the
// right. Nothing else — a bar panel is a glance, not a workspace.
Panel {
  id: root
  moduleName: "omacharts.watchlist"
  ipcTarget: "omacharts.watchlist"
  // Its own handler rather than the base one, so the app can push a refresh
  // the moment the watchlist changes instead of leaving the bar a timer
  // behind. Also lets the panel be opened from a keybinding:
  //   omarchy-shell omacharts.watchlist toggle
  manageIpc: false

  property int selectedIndex: 0

  // The bar lays widgets out by their implicit size. Without this the root is
  // zero by zero, the button anchored to it is too, and the icon never draws.
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // Gone with the package: the bar gives a hidden module no width, so there
  // is no gap where it was, the way Omarchy's own update widget disappears.
  readonly property bool gone: service.missing
  visible: !gone

  // Stands in until the host has built the real one, so every binding below
  // can read the same shape rather than guarding for null.
  QtObject {
    id: dummyService
    property var sections: []
    property var colors: null
    property string lastError: ""
    property bool missing: false
    property bool refreshing: false
    property double updatedAt: 0
    function refresh() {}
    function refreshIfStale() {}
    function openApp() {}
  }

  readonly property var hostedService: {
    var _ = bar && bar.shell ? bar.shell._services : null
    return ServiceHost.hostedService(bar)
  }
  readonly property var service: hostedService !== null ? hostedService : dummyService
  readonly property var rows: Model.rows(service.sections)
  readonly property int quoteCount: Model.quoteCount(rows)
  readonly property var headline: Model.headline(service.sections)

  // The icon in the bar wears the bar's own colour, like every other module
  // on it: it is part of the bar, and follows it when the bar goes
  // transparent over a window.
  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  // Everything in the panel is drawn in the app's own palette, which the app
  // derives from the desktop theme and the bar scheme chosen in the window
  // and sends with the data. One palette: a name here is the same quiet as a
  // name in the window's watchlist, and up is the colour the candles are.
  // Each colour arrives already checked as text against this panel and a
  // hovered row in it, so nothing here mixes, darkens or guesses.
  //
  // Until the first answer there are no rows to colour, and the panel's own
  // text colour stands in for all of them rather than a colour of our own.
  readonly property var palette: Model.palette(service.colors, Color.popups.text, Color.popups.background)
  readonly property color text: palette.text
  readonly property color muted: palette.muted
  readonly property color hover: palette.hover

  function colorFor(change) {
    return palette[Model.direction(change)]
  }

  function tagFor(change) {
    return palette.tags[Model.direction(change)]
  }

  function moveSelection(delta) {
    var picks = Model.selectableIndexes(rows)
    if (!picks.length) return
    var at = picks.indexOf(selectedIndex)
    if (at < 0) at = 0
    else at = Math.max(0, Math.min(picks.length - 1, at + delta))
    selectedIndex = picks[at]
  }

  onOpenedChanged: if (opened) service.refreshIfStale()
  // A panel left open, or opened from a keybinding, would float over a bar
  // with nothing in it to point at.
  onGoneChanged: if (gone) close()

  IpcHandler {
    target: "omacharts.watchlist"

    function open(): void { if (!root.gone) root.open() }
    function close(): void { root.close() }
    function toggle(): void { if (!root.gone || root.opened) root.toggle() }
    function refresh(): void { root.service.refresh() }
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    iconComponent: Component {
      Item {
        OmachartsIcon {
          anchors.centerIn: parent
          iconSize: Style.space(13)
          color: root.foreground
        }
      }
    }
    tooltipText: root.service.refreshing
      ? "Updating the watchlist"
      : (root.headline
        ? root.headline.display + " " + Model.formatPercent(root.headline.changePct)
        : "Omacharts watchlist")
    onPressed: function (buttonCode) {
      if (buttonCode === Qt.RightButton || buttonCode === Qt.MiddleButton) root.service.refresh()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(330))
    // Asked for what the rows want, capped — the scroller takes it from there.
    contentHeight: panel.fittedContentHeight(
      Math.min(rowsColumn.implicitHeight + Style.space(100), Style.space(620)),
      Style.space(620))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function (dx, dy) { if (dy !== 0) root.moveSelection(dy) }
      onActivateRequested: {
        var picked = root.rows[root.selectedIndex]
        if (picked && !picked.header) root.service.openApp(picked.symbol, picked.suffix)
        else root.service.openApp()
        root.close()
      }
      onCloseRequested: root.close()
      onTabRequested: function (direction) { root.switchPanel(direction) }
      onTextKey: function (text) { if (text === "r" || text === "R") root.service.refresh() }

      ColumnLayout {
        id: content
        anchors.fill: parent
        spacing: Style.space(6)

        RowLayout {
          Layout.fillWidth: true

          // The name of the app, and the way into it. It was a title doing
          // nothing while a whole row at the bottom existed to say the same
          // thing; this is one control instead of a label and a footer.
          //
          // The mark sits at the left edge of the rows below it, and the
          // highlight bleeds past it exactly as a row's does, so the panel
          // reads as one column with a heading rather than a label that
          // happens to be above a list.
          Item {
            implicitWidth: titleRow.implicitWidth
            implicitHeight: titleRow.implicitHeight + Style.space(6)

            Rectangle {
              anchors.fill: parent
              anchors.leftMargin: -Style.space(6)
              anchors.rightMargin: -Style.space(6)
              radius: Style.space(6)
              color: root.hover
              opacity: openHover.hovered ? 1 : 0
              Behavior on opacity { NumberAnimation { duration: 90 } }
            }

            HoverHandler { id: openHover }
            TapHandler { onTapped: { root.service.openApp(); root.close() } }

            RowLayout {
              id: titleRow
              anchors.centerIn: parent
              spacing: Style.space(7)

              OmachartsIcon {
                Layout.alignment: Qt.AlignVCenter
                iconSize: Style.space(15)
                color: root.text
              }

              Text {
                id: openLabel
                text: "Omacharts"
                font.family: root.fontFamily
                font.pixelSize: Style.space(14)
                font.bold: true
                color: root.text
              }
            }
          }

          Item { Layout.fillWidth: true }

          // A spinner rather than the word "updating": a word that long,
          // appearing and disappearing beside the title, is more movement than
          // the thing it is reporting.
          //
          // Drawn from rectangles rather than with BusyIndicator, which takes
          // its appearance from a Qt Controls style the shell does not
          // necessarily load — and an indicator that renders as nothing is
          // worse than the word it replaced. Eight dots around a circle, each
          // fainter than the one before, turning: the shape everything uses,
          // and nothing here can fail to draw.
          Item {
            id: spinner
            visible: root.service.refreshing
            implicitWidth: Style.space(13)
            implicitHeight: Style.space(13)
            Layout.alignment: Qt.AlignVCenter

            Repeater {
              model: 8
              Rectangle {
                width: Style.space(2.5)
                height: width
                radius: width / 2
                color: root.muted
                opacity: 0.25 + 0.75 * (index / 7)
                x: spinner.width / 2 - width / 2
                   + Math.cos(index * Math.PI / 4) * (spinner.width / 2 - width / 2)
                y: spinner.height / 2 - height / 2
                   + Math.sin(index * Math.PI / 4) * (spinner.height / 2 - height / 2)
              }
            }

            RotationAnimation on rotation {
              from: 0
              to: 360
              duration: 1200
              loops: Animation.Infinite
              running: spinner.visible
            }
          }
        }

        Text {
          Layout.fillWidth: true
          visible: root.service.lastError !== ""
          text: root.service.lastError
          wrapMode: Text.WordWrap
          font.family: root.fontFamily
          font.pixelSize: Style.space(11)
          color: root.muted
        }

        Text {
          Layout.fillWidth: true
          visible: root.rows.length === 0 && root.service.lastError === ""
          text: "Nothing on the watchlist yet."
          font.family: root.fontFamily
          font.pixelSize: Style.space(12)
          color: root.muted
        }

        // A watchlist outgrows any panel worth putting on a bar, so the rows
        // scroll and the title and the way out stay where you left them.
        ScrollView {
          Layout.fillWidth: true
          Layout.fillHeight: true
          clip: true
          ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

          ColumnLayout {
            id: rowsColumn
            width: Math.max(0, parent.width)
            spacing: Style.space(6)

        Repeater {
          model: root.rows

          delegate: Item {
            Layout.fillWidth: true
            implicitHeight: modelData.header ? Style.space(24) : Style.space(40)

            Text {
              visible: !!modelData && !!modelData.header
              anchors.left: parent.left
              anchors.bottom: parent.bottom
              anchors.bottomMargin: Style.space(4)
              text: modelData.header ? modelData.name.toUpperCase() : ""
              font.family: root.fontFamily
              font.pixelSize: Style.space(10)
              font.letterSpacing: 0.6
              color: root.muted
            }

            Rectangle {
              anchors.fill: parent
              anchors.leftMargin: -Style.space(6)
              anchors.rightMargin: -Style.space(6)
              radius: Style.space(6)
              visible: !!modelData && !modelData.header
              color: root.hover
              opacity: index === root.selectedIndex || hover.hovered ? 1 : 0
              Behavior on opacity { NumberAnimation { duration: 90 } }
            }

            HoverHandler {
              id: hover
              enabled: !!modelData && !modelData.header
              onHoveredChanged: if (hovered) root.selectedIndex = index
            }

            TapHandler {
              enabled: !!modelData && !modelData.header
              onTapped: {
                root.service.openApp(modelData.symbol, modelData.suffix)
                root.close()
              }
            }

            RowLayout {
              // Coerced: a Repeater hands the delegate an undefined modelData
              // for an instant while the model is being replaced, and QML
              // complains once per row every time the list refreshes.
              visible: !modelData || !modelData.header
              anchors.fill: parent
              spacing: Style.space(8)

              // The name is the only part of the row that may be any length,
              // so it is the only part that gives way. Everything to its right
              // keeps its size and the name elides into whatever is left —
              // otherwise a long one ("CBOE Volatility Index") pushes the
              // percentage pill off the edge of the panel.
              ColumnLayout {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                spacing: 0
                Text {
                  Layout.fillWidth: true
                  text: modelData.display || ""
                  elide: Text.ElideRight
                  font.family: root.fontFamily
                  font.pixelSize: Style.space(13)
                  font.bold: true
                  color: root.text
                }
                Text {
                  Layout.fillWidth: true
                  text: modelData.name || ""
                  elide: Text.ElideRight
                  font.family: root.fontFamily
                  font.pixelSize: Style.space(10)
                  color: root.muted
                }
              }

              Sparkline {
                // Coerced rather than left as the array itself: a row with no
                // series hands QML an undefined where it wants a bool, and it
                // complains once per row per repaint.
                visible: !!modelData.spark && modelData.spark.length > 1
                Layout.preferredWidth: Style.space(52)
                Layout.preferredHeight: Style.space(22)
                points: modelData.spark || []
                stroke: root.colorFor(modelData.change)
              }

              Text {
                text: Model.formatPrice(modelData.last, modelData.kind)
                font.family: root.fontFamily
                font.pixelSize: Style.space(13)
                color: modelData.hasQuote === true ? root.text : root.muted
              }

              // The move, as a pill: the chart's last-price tag, filled in
              // the direction's colour with the figure on it. The pill is the
              // thing you actually read, which is why it is the only filled
              // object on the row. Fill and label arrive as a pair from the
              // app, the label already picked to read on the fill, so a theme
              // whose up or down is dark, or mid-tone, still gets a percentage
              // somebody can read.
              Rectangle {
                readonly property var tag: root.tagFor(modelData.change)
                visible: modelData.hasQuote === true
                implicitWidth: Math.max(Style.space(58), percent.implicitWidth + Style.space(12))
                implicitHeight: Style.space(20)
                radius: Style.space(5)
                color: tag.fill

                Text {
                  id: percent
                  anchors.centerIn: parent
                  text: Model.formatPercent(modelData.changePct)
                  font.family: root.fontFamily
                  font.pixelSize: Style.space(11)
                  font.bold: true
                  color: parent.tag.label
                }
              }
            }
          }
        }
          }
        }

      }
    }
  }
}
