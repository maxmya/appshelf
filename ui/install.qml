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
    property string status: "Reading AppImage…"
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
        if (busy || !preview) return;
        busy = true; failed = false; status = "Copying and installing…";
        backend.write(JSON.stringify({command: "install", path: String(preview.path)}) + "\n");
    }
    function receive(message) {
        if (message.event === "quit") { Quickshell.execDetached(["kill", String(Quickshell.processId)]); return; }
        if (message.event === "ready") {
            ready = true;
            if (!target) { failed = true; status = "No AppImage was given to install"; return; }
            backend.write(JSON.stringify({command: "inspect", path: String(target)}) + "\n");
            return;
        }
        busy = false;
        if (!message.ok) { failed = true; status = message.error; return; }
        if (message.command === "inspect") { preview = message.result; status = ""; return; }
        if (message.command === "install") {
            done = true;
            status = "Installed · original file kept";
            closeTimer.start();
        }
    }

    Timer { id: closeTimer; interval: 1100; onTriggered: root.quit() }

    Process {
        id: backend
        command: [Quickshell.env("APPSHELF_BACKEND"), "--backend"]
        running: true
        stdinEnabled: true
        stdout: SplitParser {
            onRead: data => {
                try { root.receive(JSON.parse(data)); }
                catch (error) { root.failed = true; root.busy = false; root.status = "Backend response error: " + error; }
            }
        }
        stderr: StdioCollector { onStreamFinished: if (text) console.warn(text) }
        onExited: (code, exitStatus) => {
            if (!root.quitting) { root.ready = false; root.busy = false; root.failed = true; root.status = "Backend stopped. Close and reopen AppShelf."; }
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
        implicitWidth: Theme.scale(520)
        implicitHeight: Theme.scale(300)
        minimumSize: Qt.size(Theme.scale(420), Theme.scale(260))
        maximumSize: Qt.size(Theme.scale(760), Theme.scale(420))
        color: Theme.background

        Shortcut { sequence: "Escape"; enabled: !root.busy; onActivated: root.quit() }
        Shortcut { sequences: ["Return", "Enter", "Ctrl+Return"]; enabled: !root.busy && !root.done && !!root.preview; onActivated: root.install() }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Theme.scale(24)
            spacing: Theme.scale(14)

            ShelfText {
                text: "Install AppImage"
                font.pixelSize: Theme.fontSize + Theme.scale(6)
                font.bold: true
            }
            ShelfText {
                Layout.fillWidth: true
                visible: !!root.preview
                text: root.preview ? root.preview.name : ""
                color: Theme.accent
                font.pixelSize: Theme.fontSize + Theme.scale(2)
                wrapMode: Text.Wrap
            }
            ShelfText {
                Layout.fillWidth: true
                text: root.preview ? root.preview.path : root.target
                color: Theme.secondary
                font.pixelSize: Theme.smallSize
                wrapMode: Text.WrapAnywhere
                maximumLineCount: 2
                elide: Text.ElideMiddle
            }
            ShelfText {
                Layout.fillWidth: true
                visible: !!root.preview
                text: root.preview ? root.size(root.preview.size) + " · " + root.preview.format + " · FUSE-free" : ""
                color: Theme.secondary
                font.pixelSize: Theme.smallSize
            }
            ShelfText {
                Layout.fillWidth: true
                visible: !!root.preview && !!root.preview.note
                text: root.preview ? root.preview.note : ""
                color: Theme.secondary
                font.pixelSize: Theme.smallSize
                wrapMode: Text.WordWrap
            }
            ShelfText {
                Layout.fillWidth: true
                visible: !!root.status
                text: root.status
                color: root.failed ? Theme.danger : Theme.accent
                wrapMode: Text.WordWrap
            }
            Item { Layout.fillHeight: true }
            ShelfText {
                Layout.fillWidth: true
                visible: !!root.preview && !root.done
                text: "A managed copy and launcher are added to your shelf. The original file stays where it is. Only install applications you trust."
                color: Theme.secondary
                font.pixelSize: Theme.smallSize
                wrapMode: Text.WordWrap
                lineHeight: 1.35
            }
            RowLayout {
                Layout.fillWidth: true
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
