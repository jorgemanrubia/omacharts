import QtQuick
import Quickshell
import Quickshell.Io
import "Model.js" as Model

// Polling omacharts for the watchlist.
//
// One service for the whole shell, however many monitors have a bar: three
// bars asking the same question three times over is three times the requests
// for one answer.
Item {
  id: root

  property var settings: ({})
  property var sections: []
  property var colors: null
  property string lastError: ""
  property double updatedAt: 0
  readonly property bool refreshing: watchlistProcess.running

  readonly property int refreshIntervalSec: {
    var value = parseInt(String(settings && settings.refreshIntervalSec
      ? settings.refreshIntervalSec : 120), 10)
    return isFinite(value) ? Math.max(30, Math.min(1800, value)) : 120
  }

  property string _output: ""
  property string _error: ""

  function refresh() {
    if (watchlistProcess.running) return
    watchlistProcess.command = ["omacharts", "watchlist", "--refresh"]
    watchlistProcess.running = true
  }

  // Opening the panel is a reason to ask again — but asking a second after
  // the timer did is a wasted request, and a few seconds of staleness is not
  // worth one.
  function refreshIfStale() {
    if (Date.now() - updatedAt > 15000) refresh()
  }

  // The app is single-instance and handles its command line, so this focuses
  // the window that is already open — and switches it to the symbol asked for
  // rather than leaving it wherever it was. Detached, because with no window
  // open this launch *is* the window: a Process holding it queues every later
  // click until the window closes, and dies with the shell.
  function openApp(symbol, suffix) {
    var command = ["omacharts"]
    if (symbol) {
      command.push(symbol)
      if (suffix) command.push(suffix)
    }
    Quickshell.execDetached(command)
  }

  Component.onCompleted: refresh()

  Timer {
    interval: root.refreshIntervalSec * 1000
    repeat: true
    running: true
    onTriggered: root.refresh()
  }

  Process {
    id: watchlistProcess
    running: false
    command: []
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root._output = text
    }
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root._error = text
    }
    onExited: function (code) {
      if (code !== 0) {
        root.lastError = root._error.trim() || "omacharts is not on PATH"
        return
      }
      var parsed = Model.parse(root._output)
      if (parsed.error) {
        root.lastError = parsed.error
        return
      }
      // Replaced only on a good answer: a failed refresh should leave the
      // last quotes on screen rather than blanking the bar.
      root.sections = parsed.sections
      if (parsed.colors) root.colors = parsed.colors
      root.lastError = ""
      root.updatedAt = Date.now()
    }
  }
}
