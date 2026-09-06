//@ pragma AppId org.omarchy.appshelf
//@ pragma ShellId appshelf
//@ pragma NativeTextRendering
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root
    property var apps: []
    property var discovered: []
    property var allApps: apps.concat(discovered)
    property string importSource: ""
    property var preview: null
    property var removal: null
    property string editingId: ""
    property bool busy: true
    property bool ready: false
    property bool quitting: false
    property bool failed: false
    property string status: "Opening your shelf…"
    property string selectedId: ""
    property var filtered: allApps.filter(a => a.name.toLowerCase().includes(search.text.toLowerCase()))
    property var selected: allApps.find(a => a.id === selectedId) || null
    readonly property bool modalOpen: installDialog.opened || removeDialog.opened || settingsDialog.opened || helpDialog.opened

    function size(bytes) {
        return bytes >= 1073741824 ? (bytes / 1073741824).toFixed(1) + " GB" : (bytes / 1048576).toFixed(1) + " MB";
    }
    function send(command, values) {
        if (!ready || busy) return;
        busy = true;
        failed = false;
        status = ({inspect: "Reading AppImage…", install: "Copying and installing…", uninstall: "Removing application…", configure: "Saving launch settings…", list: "Refreshing…", reveal: "Opening file manager…", launch: "Starting application…"})[command] || "Working…";
        backend.write(JSON.stringify(Object.assign({command: command}, values || {})) + "\n");
    }
    function inspect(path, importing) {
        if (path) { importSource = importing ? String(path) : ""; send("inspect", {path: String(path)}); }
    }
    function activateSelected() {
        if (!selected || busy) return;
        if (selected.unmanaged) inspect(selected.path, true);
        else send("launch", {id: selected.id});
    }
    function editSelected() {
        if (!selected || selected.unmanaged || busy) return;
        editingId = selected.id;
        settingsEditor.load(selected);
        failed = false;
        settingsDialog.open();
    }
    function revealSelected() {
        send("reveal", selected ? (selected.unmanaged ? {path: selected.path} : {id: selected.id}) : {});
    }
    function removeSelected() {
        if (!selected || selected.unmanaged || busy) return;
        removal = selected; failed = false; removeDialog.open();
    }
    function focusList() {
        list.forceActiveFocus();
        if (filtered.length && !selected) { list.currentIndex = 0; selectedId = filtered[0].id; }
    }
    function quit() {
        if (quitting) return;
        quitting = true;
        if (ready) backend.write('{"command":"quit"}\n');
        else Quickshell.execDetached(["kill", String(Quickshell.processId)]);
    }
    function receive(message) {
        if (message.event === "quit") {
            Quickshell.execDetached(["kill", String(Quickshell.processId)]);
            return;
        }
        busy = false;
        if (message.apps !== undefined) apps = message.apps;
        if (message.discovered !== undefined) discovered = message.discovered;
        if (message.event === "ready") {
            ready = true;
            status = "Ready";
            focusList();
            const initial = Quickshell.env("APPSHELF_OPEN");
            if (initial) inspect(initial);
            return;
        }
        if (!message.ok) {
            failed = true;
            status = message.error;
            return;
        }
        if (message.command === "inspect") { preview = Object.assign(message.result, {external: !!importSource}); installDialog.open(); }
        if (message.command === "install") {
            selectedId = message.result.id;
            installDialog.close();
            preview = null;
        }
        if (message.command === "uninstall") { removeDialog.close(); removal = null; selectedId = ""; }
        if (message.command === "configure") settingsDialog.close();
        status = ({inspect: "Review this AppImage before installing", install: "Installed · original download kept", uninstall: "Application removed · personal data kept", configure: "Launch settings saved", launch: "Launch requested", reveal: "Opened in file manager", list: "Shelf refreshed"})[message.command] || "Ready";
    }

    Process {
        id: backend
        command: [Quickshell.env("APPSHELF_BACKEND"), "--backend"]
        running: true
        stdinEnabled: true
        stdout: SplitParser {
            onRead: data => {
                try { root.receive(JSON.parse(data)); }
                catch (error) { root.failed = true; root.status = "Backend response error: " + error; root.busy = false; }
            }
        }
        stderr: StdioCollector { onStreamFinished: if (text) console.warn(text) }
        onExited: (code, exitStatus) => {
            if (!root.quitting) { root.ready = false; root.busy = false; root.failed = true; root.status = "Backend stopped. Close and reopen AppShelf."; }
        }
    }
    Connections { target: Quickshell; function onLastWindowClosed() { root.quit(); } }
    // Read-only introspection for integration tests and diagnostics.
    IpcHandler {
        target: "appshelf"
        function state(): string {
            return JSON.stringify({ready: root.ready, busy: root.busy, count: root.apps.length, discovered: root.discovered.length, selected: root.selectedId, modal: root.modalOpen, status: root.status,
                                   preview: root.preview, background: String(Theme.background), accent: String(Theme.accent),
                                   font: Theme.family, fontSize: Theme.fontSize});
        }
    }

    FloatingWindow {
        id: window
        title: "AppShelf"
        implicitWidth: 940
        implicitHeight: 600
        minimumSize: Qt.size(680, 440)
        color: Theme.background

        Shortcut { sequence: "Ctrl+O"; enabled: !root.busy && !root.modalOpen; onActivated: picker.open() }
        Shortcut { sequence: "Ctrl+F"; enabled: !root.modalOpen; onActivated: search.forceActiveFocus() }
        Shortcut { sequence: "Ctrl+L"; enabled: !root.modalOpen; onActivated: root.focusList() }
        Shortcut { sequence: "Ctrl+R"; enabled: !root.busy && !root.modalOpen; onActivated: root.send("list") }
        Shortcut { sequence: "Ctrl+E"; enabled: !root.modalOpen; onActivated: root.editSelected() }
        Shortcut { sequence: "Ctrl+Shift+F"; enabled: !root.modalOpen; onActivated: root.revealSelected() }
        Shortcut { sequence: "F1"; enabled: !root.modalOpen; onActivated: helpDialog.open() }
        Shortcut { sequence: "Ctrl+Return"; enabled: installDialog.opened && !root.busy; onActivated: root.send("install", {path: root.preview.path}) }
        Shortcut { sequence: "Ctrl+S"; enabled: settingsDialog.opened && !root.busy; onActivated: saveSettings.clicked() }
        Shortcut { sequence: "Ctrl+Q"; onActivated: root.quit() }

        ColumnLayout {
            anchors.fill: parent
            spacing: 0
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: Math.max(60, Theme.rowHeight + 12)
                color: Theme.surface
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 22; anchors.rightMargin: 22
                    spacing: 12
                    ShelfText { text: "▤"; color: Theme.accent; font.pixelSize: 26 }
                    ShelfText { text: "AppShelf"; font.bold: true; font.pixelSize: Theme.fontSize + 3 }
                    Item { Layout.fillWidth: true }
                    ShelfButton { text: "?"; Accessible.name: "Keyboard shortcuts (F1)"; onClicked: helpDialog.open() }
                    ShelfButton { text: "Show in Flea"; enabled: root.ready && !root.busy; onClicked: root.send("reveal") }
                    ShelfButton { text: "+ Install AppImage"; primary: true; enabled: root.ready && !root.busy; onClicked: picker.open() }
                }
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 20
                spacing: 16
                ShelfText { text: "APPLICATIONS"; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                ShelfText { text: root.apps.length + " managed · " + root.discovered.length + " found"; color: Theme.accent; font.pixelSize: Theme.smallSize }
                Item { Layout.fillWidth: true }
                TextField {
                    id: search
                    Layout.preferredWidth: 260
                    implicitHeight: 34
                    placeholderText: "Search your shelf  /  Ctrl+F"
                    Accessible.name: "Search applications"
                    color: Theme.foreground
                    placeholderTextColor: Theme.secondary
                    selectionColor: Theme.accent
                    selectedTextColor: Theme.background
                    font.family: Theme.family
                    font.pixelSize: Theme.fontSize
                    leftPadding: 10
                    background: Rectangle { color: Theme.surface; border.width: 1; border.color: search.activeFocus ? Theme.accent : Theme.line }
                    Keys.onDownPressed: { list.forceActiveFocus(); if (list.count) { list.currentIndex = 0; root.selectedId = root.filtered[0].id; } }
                    Keys.onEscapePressed: { text = ""; list.forceActiveFocus(); }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0
                Item {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    ListView {
                        id: list
                        anchors.fill: parent
                        anchors.margins: 12
                        model: root.filtered
                        clip: true
                        spacing: 4
                        focus: true
                        keyNavigationEnabled: true
                        onCurrentIndexChanged: if (currentIndex >= 0 && currentIndex < root.filtered.length) root.selectedId = root.filtered[currentIndex].id
                        Keys.onReturnPressed: root.activateSelected()
                        Keys.onDeletePressed: root.removeSelected()
                        Keys.onPressed: event => {
                            if (event.key === Qt.Key_J) { currentIndex = Math.min(count - 1, currentIndex + 1); event.accepted = true; }
                            else if (event.key === Qt.Key_K) { currentIndex = Math.max(0, currentIndex - 1); event.accepted = true; }
                            else if (event.key === Qt.Key_Slash) { search.forceActiveFocus(); event.accepted = true; }
                            else if (event.key === Qt.Key_Escape) { search.text = ""; event.accepted = true; }
                        }
                        ScrollBar.vertical: ScrollBar {}
                        delegate: Rectangle {
                            id: appRow
                            required property var modelData
                            required property int index
                            Accessible.role: Accessible.ListItem
                            Accessible.name: modelData.name + (modelData.unmanaged ? ", discovered, Enter to add" : ", managed, Enter to launch")
                            width: list.width
                            height: Theme.rowHeight + 12
                            color: root.selectedId === modelData.id ? Qt.alpha(Theme.accent, 0.12) : (mouse.containsMouse ? Theme.surface : "transparent")
                            border.width: root.selectedId === modelData.id ? 1 : 0
                            border.color: Qt.alpha(Theme.accent, 0.5)
                            RowLayout {
                                anchors.fill: parent
                                anchors.margins: 12
                                spacing: 14
                                Rectangle {
                                    width: 36; height: 36; color: Theme.surface
                                    Image { id: icon; anchors.fill: parent; source: appRow.modelData.icon; fillMode: Image.PreserveAspectFit; sourceSize: Qt.size(36, 36) }
                                    ShelfText { anchors.centerIn: parent; visible: icon.status !== Image.Ready; text: appRow.modelData.name.charAt(0).toUpperCase(); color: Theme.accent; font.pixelSize: 20 }
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true; spacing: 4
                                    ShelfText { text: appRow.modelData.name; Layout.fillWidth: true; elide: Text.ElideRight; font.bold: true }
                                    ShelfText { text: appRow.modelData.missing ? "File missing" : (appRow.modelData.unmanaged ? "Found  ·  " : "") + appRow.modelData.format + "  ·  " + root.size(appRow.modelData.size); color: appRow.modelData.missing ? Theme.danger : Theme.secondary; font.pixelSize: Theme.smallSize }
                                }
                                ShelfText { text: "↗"; color: Theme.secondary }
                            }
                            MouseArea {
                                id: mouse
                                anchors.fill: parent
                                hoverEnabled: true
                                onClicked: { list.currentIndex = appRow.index; root.selectedId = appRow.modelData.id; list.forceActiveFocus(); }
                                onDoubleClicked: { root.selectedId = appRow.modelData.id; root.activateSelected(); }
                            }
                        }
                    }
                    ColumnLayout {
                        anchors.centerIn: parent
                        width: parent.width - 70
                        visible: root.filtered.length === 0
                        spacing: 18
                        ShelfText { Layout.alignment: Qt.AlignHCenter; text: "▤"; color: Theme.accent; font.pixelSize: 48 }
                        ShelfText { Layout.alignment: Qt.AlignHCenter; text: search.text ? "No matching applications" : "A place for your AppImages"; font.pixelSize: Theme.fontSize + 4; font.bold: true }
                        ShelfText { Layout.fillWidth: true; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.WordWrap; text: search.text ? "Try another name." : "Drop an AppImage here, choose a file,\nor open one from Flea."; color: Theme.secondary; lineHeight: 1.5 }
                        ShelfButton { Layout.alignment: Qt.AlignHCenter; visible: !search.text; text: "Choose AppImage"; enabled: root.ready && !root.busy; onClicked: picker.open() }
                    }
                    DropArea {
                        anchors.fill: parent
                        enabled: root.ready && !root.busy && !root.modalOpen
                        onDropped: drop => {
                            if (drop.hasUrls && drop.urls.length === 1) { root.inspect(drop.urls[0]); drop.acceptProposedAction(); }
                            else { root.failed = true; root.status = "Drop one local AppImage at a time."; }
                        }
                    }
                }
                Rectangle { Layout.fillHeight: true; width: 1; color: Theme.line }
                Rectangle {
                    Layout.preferredWidth: 270
                    Layout.fillHeight: true
                    color: Theme.surface
                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 22
                        spacing: 16
                        ShelfText { text: root.selected ? (root.selected.unmanaged ? "FOUND ON YOUR COMPUTER" : "ON YOUR SHELF") : "LOCAL. SIMPLE. YOURS."; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                        ShelfText { Layout.fillWidth: true; text: root.selected ? root.selected.name : "Ready when you are."; font.pixelSize: Theme.fontSize + 6; font.bold: true; wrapMode: Text.Wrap }
                        ShelfText { Layout.fillWidth: true; text: root.selected ? (root.selected.unmanaged ? root.selected.path + "\n\nAdd a managed copy to use AppShelf launch settings. The existing installation is kept." : root.size(root.selected.size) + "  ·  " + root.selected.format + "\nFUSE-free · uruntime\n\nInstalled " + new Date(root.selected.installed * 1000).toLocaleDateString() + "\nIsolation: " + root.selected.isolation) : "Keep your AppImages together and find them in your launcher.\n\nSquashFS and DwarFS.\nNo libfuse2 required."; wrapMode: Text.WrapAnywhere; lineHeight: 1.5; color: Theme.secondary }
                        ShelfButton { Layout.fillWidth: true; visible: !!root.selected; text: root.selected && root.selected.unmanaged ? "Add to AppShelf  ↵" : "Launch  ↗"; primary: true; enabled: root.ready && !root.busy && !!root.selected && !root.selected.missing; onClicked: root.activateSelected() }
                        ShelfButton { Layout.fillWidth: true; visible: !!root.selected; text: "Show in Flea"; enabled: root.ready && !root.busy; onClicked: root.revealSelected() }
                        ShelfButton { Layout.fillWidth: true; visible: !!root.selected && !root.selected.unmanaged; text: "Launch settings…"; enabled: root.ready && !root.busy; onClicked: root.editSelected() }
                        Item { Layout.fillHeight: true }
                        ShelfButton { Layout.fillWidth: true; visible: !!root.selected && !root.selected.unmanaged; text: "Uninstall…"; destructive: true; enabled: root.ready && !root.busy; onClicked: root.removeSelected() }
                        ShelfText { Layout.fillWidth: true; text: "↑↓ / j k  Navigate\nEnter     Launch / add\nCtrl+E    Settings\nF1        All shortcuts"; color: Theme.secondary; font.pixelSize: Theme.smallSize; lineHeight: 1.6 }
                    }
                }
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 12
                ShelfText { text: root.busy ? "◌" : (root.failed ? "!" : "●"); color: root.failed ? Theme.danger : Theme.accent }
                ShelfText { Layout.fillWidth: true; text: root.status; color: root.failed ? Theme.danger : Theme.secondary; elide: Text.ElideRight; font.pixelSize: Theme.smallSize }
            }
        }

        FileDialog {
            id: picker
            title: "Choose an AppImage"
            nameFilters: ["AppImages (*.AppImage *.appimage)", "All files (*)"]
            onAccepted: root.inspect(selectedFile)
        }
        Dialog {
            id: installDialog
            anchors.centerIn: parent
            width: Math.min(510, window.width - 40)
            modal: true
            padding: 24
            onClosed: root.focusList()
            closePolicy: root.busy ? Popup.NoAutoClose : Popup.CloseOnEscape
            background: Rectangle { color: Theme.background; border.color: Theme.accent; border.width: 1 }
            contentItem: ColumnLayout {
                spacing: 18
                ShelfText { text: root.preview && root.preview.external ? "Add existing AppImage" : "Install AppImage"; font.pixelSize: Theme.fontSize + 6; font.bold: true }
                ShelfText { Layout.fillWidth: true; text: root.preview ? root.preview.name : ""; wrapMode: Text.Wrap; color: Theme.accent; font.pixelSize: Theme.fontSize + 2 }
                ShelfText { Layout.fillWidth: true; text: root.preview ? root.preview.path : ""; wrapMode: Text.WrapAnywhere; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                ShelfText { Layout.fillWidth: true; text: (root.preview ? root.size(root.preview.size) + " · " + root.preview.format + " · FUSE-free\n\n" : "") + (root.preview && root.preview.external ? "Create an AppShelf-managed copy and launcher. The existing installation, launcher, and its settings remain unchanged. You may see both launchers until you remove the old installation with its original manager." : "Add a managed copy to your shelf and create a launcher entry. The original file stays in place.") + "\n\nUse Launch settings to set environment variables and isolation before starting the app. Only install applications you trust."; wrapMode: Text.WordWrap; lineHeight: 1.4 }
                ShelfText { Layout.fillWidth: true; visible: !!root.preview && !!root.preview.note; text: root.preview ? root.preview.note : ""; wrapMode: Text.WordWrap; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                ShelfText { Layout.fillWidth: true; visible: root.failed || root.busy; text: root.status; wrapMode: Text.WordWrap; color: root.failed ? Theme.danger : Theme.accent }
                RowLayout {
                    Layout.fillWidth: true
                    Item { Layout.fillWidth: true }
                    ShelfButton { text: "Cancel"; enabled: !root.busy; onClicked: installDialog.close() }
                    ShelfButton { text: root.busy ? "Installing…" : "Install"; primary: true; enabled: !root.busy && root.ready; onClicked: root.send("install", {path: root.preview.path}) }
                }
            }
        }
        Dialog {
            id: settingsDialog
            onClosed: root.focusList()
            anchors.centerIn: parent
            width: Math.min(540, window.width - 40)
            height: Math.min(530, window.height - 30)
            modal: true
            padding: 20
            closePolicy: root.busy ? Popup.NoAutoClose : Popup.CloseOnEscape
            background: Rectangle { color: Theme.background; border.color: Theme.accent; border.width: 1 }
            contentItem: ColumnLayout {
                spacing: 14
                ShelfText { text: "Launch settings"; font.bold: true; font.pixelSize: Theme.fontSize + 5 }
                ScrollView {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    contentWidth: availableWidth
                    AppSettings { id: settingsEditor; width: parent.width; enabled: !root.busy }
                }
                ShelfText { Layout.fillWidth: true; visible: root.failed; text: root.status; wrapMode: Text.WordWrap; color: Theme.danger }
                RowLayout {
                    Layout.fillWidth: true
                    Item { Layout.fillWidth: true }
                    ShelfButton { text: "Cancel"; enabled: !root.busy; onClicked: settingsDialog.close() }
                    ShelfButton {
                        id: saveSettings
                        text: "Save settings"; primary: true; enabled: root.ready && !root.busy
                        onClicked: {
                            try { root.send("configure", {id: root.editingId, environment: settingsEditor.environment(), isolation: settingsEditor.isolation}); }
                            catch (error) { root.failed = true; root.status = String(error); }
                        }
                    }
                }
            }
        }
        Dialog {
            id: removeDialog
            onClosed: root.focusList()
            anchors.centerIn: parent
            width: Math.min(470, window.width - 40)
            modal: true
            padding: 24
            closePolicy: root.busy ? Popup.NoAutoClose : Popup.CloseOnEscape
            background: Rectangle { color: Theme.background; border.color: Theme.danger; border.width: 1 }
            contentItem: ColumnLayout {
                spacing: 20
                ShelfText { text: "Remove from your shelf?"; font.bold: true; font.pixelSize: Theme.fontSize + 4 }
                ShelfText { Layout.fillWidth: true; text: root.removal ? root.removal.name : ""; wrapMode: Text.Wrap; color: Theme.accent }
                ShelfText { Layout.fillWidth: true; text: "This deletes the managed AppImage copy, its icon and launcher entry. Your original download and personal application data are kept."; wrapMode: Text.WordWrap; lineHeight: 1.5 }
                ShelfText { Layout.fillWidth: true; visible: root.failed; text: root.status; wrapMode: Text.WordWrap; color: Theme.danger }
                RowLayout {
                    Layout.fillWidth: true
                    Item { Layout.fillWidth: true }
                    ShelfButton { text: "Keep app"; enabled: !root.busy; onClicked: removeDialog.close() }
                    ShelfButton { text: root.busy ? "Removing…" : "Uninstall"; destructive: true; enabled: root.ready && !root.busy; onClicked: root.send("uninstall", {id: root.removal.id}) }
                }
            }
        }
        Dialog {
            id: helpDialog
            anchors.centerIn: parent
            width: Math.min(500, window.width - 40)
            modal: true
            padding: 22
            onClosed: root.focusList()
            background: Rectangle { color: Theme.background; border.color: Theme.accent; border.width: 1 }
            contentItem: ColumnLayout {
                spacing: 18
                ShelfText { text: "Your keyboard, your shelf"; font.bold: true; font.pixelSize: Theme.fontSize + 4 }
                ShelfText { Layout.fillWidth: true; text: "↑ ↓ / j k      Move through applications\nHome / End     First / last application\nEnter          Launch or add selected app\n/ or Ctrl+F    Search\nCtrl+L         Focus application list\nCtrl+O         Choose an AppImage\nCtrl+R         Rescan installed AppImages\nCtrl+E         Edit launch settings\nCtrl+Shift+F   Show selected app in Flea\nDelete         Confirm uninstall\nCtrl+Enter     Install from preview\nCtrl+S         Save launch settings\nTab / Shift+Tab  Move between controls\nSpace / Enter  Activate focused control\nEscape         Close dialog / clear search\nF1             This guide\nCtrl+Q         Quit"; lineHeight: 1.7; font.pixelSize: Theme.fontSize }
                ShelfButton { Layout.alignment: Qt.AlignRight; text: "Back to shelf"; onClicked: helpDialog.close() }
            }
        }
    }
}
