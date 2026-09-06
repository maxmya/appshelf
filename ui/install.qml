//@ pragma AppId org.omarchy.appshelf
//@ pragma ShellId appshelf-install
//@ pragma NativeTextRendering
// Compact installer shown when an AppImage is opened from the file manager.
// Double-clicking a file should not have to load the whole shelf.
import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root

    property var preview: null
    property bool ready: false
    property bool busy: false
    property bool failed: false
    property bool done: false
    property bool quitting: false
    property string status: ""
    readonly property string target: Quickshell.env("APPSHELF_OPEN")

    function size(bytes) {
        if (!bytes) return "";
        const units = ["B", "KB", "MB", "GB"];
        let value = bytes, unit = 0;
        while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
        return value.toFixed(value < 10 && unit > 0 ? 1 : 0) + " " + units[unit];
    }
    function quit() {
        if (quitting) return;
        quitting = true;
        if (ready) backend.write('{"command":"quit"}\n');
        else Quickshell.execDetached(["kill", String(Quickshell.processId)]);
    }
    function install() {
        if (busy || done || !preview) return;
        busy = true; failed = false; status = "";
        backend.write(JSON.stringify({command: "install", path: String(preview.path)}) + "\n");
    }
    function receive(message) {
        if (message.event === "quit") { Quickshell.execDetached(["kill", String(Quickshell.processId)]); return; }
        if (message.event === "ready") {
            ready = true;
            if (!target) { failed = true; status = "No AppImage given"; return; }
            backend.write(JSON.stringify({command: "inspect", path: String(target)}) + "\n");
            return;
        }
        busy = false;
        if (!message.ok) { failed = true; status = message.error; return; }
        if (message.command === "inspect") { preview = message.result; return; }
        if (message.command === "install") { done = true; closeTimer.start(); }
    }

    Timer { id: closeTimer; interval: 900; onTriggered: root.quit() }

    Process {
        id: backend
        command: [Quickshell.env("APPSHELF_BACKEND"), "--backend"]
        running: true
        stdinEnabled: true
        stdout: SplitParser {
            onRead: data => {
                try { root.receive(JSON.parse(data)); }
                catch (error) { root.failed = true; root.busy = false; root.status = "Backend error: " + error; }
            }
        }
        stderr: StdioCollector { onStreamFinished: if (text) console.warn(text) }
        onExited: (code, exitStatus) => {
            if (!root.quitting) { root.ready = false; root.busy = false; root.failed = true; root.status = "Backend stopped"; }
        }
    }
    Connections { target: Quickshell; function onLastWindowClosed() { root.quit(); } }

    IpcHandler {
        target: "appshelf-install"
        function state(): string {
            return JSON.stringify({ready: root.ready, busy: root.busy, failed: root.failed, done: root.done,
                                   status: root.status, preview: root.preview, target: root.target});
        }
        function install(): bool { root.install(); return root.busy; }
    }

    FloatingWindow {
        id: window
        title: "Install AppImage"
        color: Theme.background
        implicitWidth: Theme.scale(460)
        implicitHeight: content.implicitHeight + content.anchors.margins * 2
        minimumSize: Qt.size(Theme.scale(380), implicitHeight)
        maximumSize: Qt.size(Theme.scale(720), implicitHeight)

        Shortcut { sequence: "Escape"; enabled: !root.busy; onActivated: root.quit() }
        Shortcut { sequences: ["Return", "Enter"]; enabled: !root.busy && !root.done && !!root.preview; onActivated: root.install() }

        ColumnLayout {
            id: content
            anchors.fill: parent
            anchors.margins: Theme.scale(20)
            spacing: Theme.scale(16)

            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.scale(14)

                Rectangle {
                    implicitWidth: Theme.scale(46)
                    implicitHeight: Theme.scale(46)
                    radius: Math.min(Theme.radius, Theme.scale(10))
                    color: icon.status === Image.Ready ? "transparent" : Qt.alpha(Theme.accent, 0.14)
                    border.width: icon.status === Image.Ready ? 0 : 1
                    border.color: Qt.alpha(Theme.accent, 0.35)

                    ShelfText {
                        anchors.centerIn: parent
                        visible: icon.status !== Image.Ready
                        text: root.preview && root.preview.name ? root.preview.name.charAt(0).toUpperCase() : "?"
                        color: Theme.accent
                        font.pixelSize: Theme.fontSize + Theme.scale(8)
                        font.bold: true
                    }
                    Image {
                        id: icon
                        anchors.fill: parent
                        anchors.margins: Theme.scale(2)
                        source: root.preview && root.preview.icon ? "file://" + root.preview.icon : ""
                        fillMode: Image.PreserveAspectFit
                        smooth: true
                        asynchronous: true
                        visible: status === Image.Ready
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Theme.scale(3)
                    ShelfText {
                        Layout.fillWidth: true
                        text: root.preview ? root.preview.name : "Reading…"
                        font.pixelSize: Theme.fontSize + Theme.scale(5)
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    ShelfText {
                        Layout.fillWidth: true
                        visible: !!root.preview
                        text: root.preview ? root.size(root.preview.size) + " · " + root.preview.format : ""
                        color: Theme.secondary
                        font.pixelSize: Theme.smallSize
                    }
                }
            }

            ShelfText {
                Layout.fillWidth: true
                text: root.preview ? root.preview.path : root.target
                color: Theme.secondary
                font.pixelSize: Theme.smallSize
                elide: Text.ElideMiddle
            }

            ShelfText {
                Layout.fillWidth: true
                visible: text !== ""
                text: root.done ? "Installed" : (root.status || (root.preview && root.preview.note ? root.preview.note : ""))
                color: root.failed ? Theme.danger : Theme.accent
                font.pixelSize: Theme.smallSize
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: Theme.scale(2)
                spacing: Theme.scale(8)
                Item { Layout.fillWidth: true }
                ShelfButton { text: root.done ? "Close" : "Cancel"; enabled: !root.busy; onClicked: root.quit() }
                ShelfButton {
                    text: root.busy ? "Installing…" : (root.done ? "Done" : "Install")
                    primary: true
                    enabled: !root.busy && !root.done && !!root.preview
                    onClicked: root.install()
                }
            }
        }
    }
}
