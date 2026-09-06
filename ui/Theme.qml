pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons

Singleton {
    id: theme
    readonly property string directory: Quickshell.env("APPSHELF_THEME_DIR") || (Quickshell.env("HOME") + "/.local/state/omarchy/current/theme")
    readonly property color background: Color.background
    readonly property color foreground: Color.foreground
    readonly property color accent: Color.accent
    readonly property color danger: Color.urgent
    readonly property color surface: Qt.tint(background, Qt.alpha(foreground, 0.035))
    readonly property color line: Qt.alpha(foreground, 0.16)
    readonly property color secondary: Qt.tint(background, Qt.alpha(foreground, 0.67))
    readonly property string family: Style.font.family
    readonly property int fontSize: Style.font.bodySmall
    readonly property int smallSize: Style.font.caption
    readonly property int gap: Style.spacing.rowGap
    readonly property int padding: Style.spacing.rowPaddingX
    readonly property int radius: Style.cornerRadius
    readonly property int rowHeight: Math.max(48, fontSize * 3.8)
    property string name: "Omarchy"
    property string lastColors: ""
    property string lastShell: ""

    function applyColors(body) {
        if (body && body !== lastColors) {
            lastColors = body;
            Color.loadColors(body);
        }
    }
    function applyShell(body) {
        if (body !== lastShell) {
            lastShell = body;
            Color.loadShell(body);
            Style.scheduleRefresh();
        }
    }
    function refresh() { colors.reload(); shellStyle.reload(); themeName.reload(); }

    FileView {
        id: colors
        path: theme.directory + "/colors.toml"
        blockLoading: true
        printErrors: false
        watchChanges: true
        onLoaded: theme.applyColors(text())
        onFileChanged: reload()
        Component.onCompleted: theme.applyColors(text())
    }
    FileView {
        id: shellStyle
        path: theme.directory + "/shell.toml"
        blockLoading: true
        printErrors: false
        watchChanges: true
        onLoaded: theme.applyShell(text())
        onFileChanged: reload()
        Component.onCompleted: theme.applyShell(text())
    }
    FileView {
        id: themeName
        path: theme.directory + "/../theme.name"
        printErrors: false
        watchChanges: true
        onLoaded: theme.name = text().trim() || "Omarchy"
        onFileChanged: theme.refresh()
    }
    // Directory replacements invalidate inode watches; polling also covers custom/manual themes.
    Timer { interval: 2000; running: true; repeat: true; onTriggered: theme.refresh() }
}
