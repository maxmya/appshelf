//@ pragma AppId org.omarchy.appshelf
//@ pragma ShellId appshelf-setup
//@ pragma NativeTextRendering
// First-run setup, shown when the AppShelf AppImage itself is run. The copy
// inside the AppImage lives on a mount that disappears with the process, so
// running it is a request to install AppShelf, not to use it from there.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root

    property var info: null
    property bool busy: false
    property bool failed: false
    property bool done: false
    property bool integrate: false
    property string status: ""

    readonly property bool installed: !!info && !!info.installed
    readonly property bool blocked: !!info && !!info.blocked
    readonly property bool upgrade: installed && info.installed_version !== info.version
    readonly property bool actionable: !!info && !busy && !blocked

    function quit() {
        Quickshell.execDetached(["kill", String(Quickshell.processId)]);
    }
    function applyState(text) {
        try {
            info = JSON.parse(text);
        } catch (error) {
            failed = true;
            status = "Could not read the install state: " + error;
            return;
        }
        // Opting out of the file association is a separate decision; start from
        // whatever is already true, and offer it on a first install.
        integrate = info.integrated || !info.installed;
    }
    function install() {
        if (!actionable || done) return;
        busy = true;
        failed = false;
        status = "";
        installer.command = [Quickshell.env("APPSHELF_BACKEND"), "--install",
                             integrate ? "--integrate" : "--no-integrate"];
        installer.running = true;
    }
    function finished(code) {
        busy = false;
        if (code === 0) {
            done = true;
            failed = false;
            status = integrate ? "Installed, and set as your AppImage opener." : "Installed.";
            return;
        }
        failed = true;
        status = errors.text.trim() || ("Install failed (exit " + code + ")");
    }
    function openInstalled() {
        Quickshell.execDetached([info.binary]);
        quit();
    }
    function openFromImage() {
        if (info && info.appimage) Quickshell.execDetached([info.appimage, "--shelf"]);
        else Quickshell.execDetached([Quickshell.env("APPSHELF_BACKEND"), "--shelf"]);
        quit();
    }

    Process {
        id: probe
        command: [Quickshell.env("APPSHELF_BACKEND"), "--setup-state"]
        running: true
        stdout: StdioCollector { onStreamFinished: root.applyState(text) }
        stderr: StdioCollector { onStreamFinished: if (text) console.warn(text) }
    }
    Process {
        id: installer
        stdout: StdioCollector { onStreamFinished: if (text) console.log(text) }
        stderr: StdioCollector { id: errors; onStreamFinished: if (text) console.warn(text) }
        onExited: (code, exitStatus) => root.finished(code)
    }
    Connections { target: Quickshell; function onLastWindowClosed() { root.quit(); } }

    IpcHandler {
        target: "appshelf-setup"
        function state(): string {
            return JSON.stringify({busy: root.busy, failed: root.failed, done: root.done,
                                   integrate: root.integrate, status: root.status, info: root.info});
        }
        function install(): bool { root.install(); return root.busy; }
    }

    FloatingWindow {
        id: window
        title: root.done ? "AppShelf installed" : (root.installed ? "Update AppShelf" : "Install AppShelf")
        color: Theme.background
        implicitWidth: Theme.scale(470)
        implicitHeight: content.implicitHeight + content.anchors.margins * 2
        minimumSize: Qt.size(Theme.scale(400), implicitHeight)
        maximumSize: Qt.size(Theme.scale(720), implicitHeight)

        Shortcut { sequence: "Escape"; enabled: !root.busy; onActivated: root.quit() }
        Shortcut {
            sequences: ["Return", "Enter"]
            enabled: !root.busy
            onActivated: root.done ? root.openInstalled() : root.install()
        }

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
                        text: "A"
                        color: Theme.accent
                        font.pixelSize: Theme.fontSize + Theme.scale(8)
                        font.bold: true
                    }
                    Image {
                        id: icon
                        anchors.fill: parent
                        anchors.margins: Theme.scale(2)
                        source: root.info && root.info.icon ? "file://" + root.info.icon : ""
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
                        text: "AppShelf"
                        font.pixelSize: Theme.fontSize + Theme.scale(5)
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    ShelfText {
                        Layout.fillWidth: true
                        visible: !!root.info
                        color: Theme.secondary
                        font.pixelSize: Theme.smallSize
                        elide: Text.ElideRight
                        text: !root.info ? ""
                            : !root.upgrade && root.installed ? "Version " + root.info.version + " · already installed"
                            : !root.installed ? "Version " + root.info.version
                            : root.info.installed_version
                                ? "Installed " + root.info.installed_version + " · this AppImage is " + root.info.version
                                : "An older version is installed · this AppImage is " + root.info.version
                    }
                }
            }

            ShelfText {
                Layout.fillWidth: true
                text: !root.info ? "Reading…"
                    : "Copies AppShelf into " + root.info.binary + " and your application launcher, so it keeps working after this AppImage is moved or deleted."
                color: Theme.secondary
                font.pixelSize: Theme.smallSize
                wrapMode: Text.WordWrap
            }

            CheckBox {
                id: opener
                Layout.fillWidth: true
                padding: 0
                leftPadding: Theme.scale(26)
                spacing: Theme.scale(10)
                hoverEnabled: true
                focusPolicy: Qt.StrongFocus
                enabled: root.actionable && !root.done
                opacity: enabled ? 1 : 0.5
                checked: root.integrate
                onToggled: root.integrate = checked

                indicator: Rectangle {
                    x: 0
                    y: Theme.scale(1)
                    implicitWidth: Theme.scale(16)
                    implicitHeight: Theme.scale(16)
                    radius: Math.min(Theme.scale(4), Theme.radius)
                    color: opener.checked ? Theme.accent : "transparent"
                    border.width: 1
                    border.color: opener.activeFocus || opener.hovered ? Theme.accent : Theme.line

                    ShelfText {
                        anchors.centerIn: parent
                        visible: opener.checked
                        text: "✓"
                        color: Theme.background
                        font.pixelSize: Theme.smallSize
                        font.bold: true
                    }
                }
                contentItem: ColumnLayout {
                    spacing: Theme.scale(2)
                    ShelfText {
                        Layout.fillWidth: true
                        text: "Open AppImages with AppShelf"
                        wrapMode: Text.WordWrap
                    }
                    ShelfText {
                        Layout.fillWidth: true
                        text: "AppImages opened from your file manager or Flea come here first. Your previous opener is saved."
                        color: Theme.secondary
                        font.pixelSize: Theme.smallSize
                        wrapMode: Text.WordWrap
                    }
                }
            }

            ShelfText {
                Layout.fillWidth: true
                visible: text !== ""
                text: root.blocked && !root.done
                        ? root.info.binary + " already exists and is not an AppShelf link. Remove it, then try again."
                        : root.status
                color: root.failed || root.blocked ? Theme.danger : Theme.accent
                font.pixelSize: Theme.smallSize
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: Theme.scale(2)
                spacing: Theme.scale(8)
                Item { Layout.fillWidth: true }
                ShelfButton {
                    text: root.done ? "Close" : "Open without installing"
                    enabled: !root.busy
                    onClicked: root.done ? root.quit() : root.openFromImage()
                }
                ShelfButton {
                    primary: true
                    enabled: root.done ? true : root.actionable
                    text: root.busy ? "Installing…"
                        : root.done ? "Open AppShelf"
                        : root.upgrade ? "Update"
                        : root.installed ? "Reinstall"
                        : "Install"
                    onClicked: root.done ? root.openInstalled() : root.install()
                }
            }
        }
    }
}
