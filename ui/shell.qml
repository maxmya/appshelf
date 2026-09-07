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
    property real detailsWidth: 270
    property bool detailsVisible: true
    property var updateResult: null
    property bool checkingUpdate: false
    property bool updatingApp: false
    property string updateFeedback: ""
    property string updateStatusType: ""
    property string activeView: "shelf"
    // AppShelf's own installation: version, tray state and autostart, refreshed
    // by the backend with every reply.
    property var preferences: null
    property string pendingAction: ""
    property var selfUpdate: null
    property string selfFeedback: ""
    property string selfStatusType: ""
    property var allUpdates: null
    property string allFeedback: ""
    readonly property bool isNarrow: (window.width / Theme.zoom) < 860
    readonly property bool isCompact: (window.width / Theme.zoom) < 680
    readonly property bool modalOpen: installDialog.opened || removeDialog.opened || helpDialog.opened
    readonly property bool trayRunning: !!(preferences && preferences.tray_running)
    readonly property bool trayAutostart: !!(preferences && preferences.service_installed)
    readonly property string selfChannel: preferences ? String(preferences.channel) : ""
    readonly property bool selfUpdatable: selfChannel === "appimage" || selfChannel === "installed"
    readonly property bool trayUpdatesPending: !!(selfUpdate && selfUpdate.has_update) || !!(allUpdates && allUpdates.updates.length)
    // A system package is installed by pacman and owned by pacman: it has no
    // managed copy, no launch settings and no update AppShelf could apply. The
    // rows still belong on the shelf, so almost every control below asks this
    // before it offers itself.
    readonly property bool selectedIsPackage: !!(selected && selected.kind === "package")
    readonly property bool removalIsPackage: !!(removal && removal.kind === "package")
    readonly property bool previewIsPackage: !!(preview && preview.kind === "package")
    readonly property int packageCount: apps.filter(a => a.kind === "package").length
    readonly property int shelfCount: apps.length - packageCount

    function isPackage(app) { return !!(app && app.kind === "package"); }

    function toggleDetails() {
        detailsVisible = !detailsVisible;
        status = detailsVisible ? "Side panel shown" : "Side panel hidden";
    }

    function zoomIn() {
        Theme.zoom = Math.min(2.0, Math.round((Theme.zoom + 0.1) * 10) / 10);
        status = "Zoom: " + Math.round(Theme.zoom * 100) + "%";
    }
    function zoomOut() {
        Theme.zoom = Math.max(0.7, Math.round((Theme.zoom - 0.1) * 10) / 10);
        status = "Zoom: " + Math.round(Theme.zoom * 100) + "%";
    }
    function resetZoom() {
        Theme.zoom = 1.0;
        status = "Zoom: 100%";
    }

    function size(bytes) {
        return bytes >= 1073741824 ? (bytes / 1073741824).toFixed(1) + " GB" : (bytes / 1048576).toFixed(1) + " MB";
    }
    function send(command, values) {
        if (!ready || busy) return;
        busy = true;
        failed = false;
        // pacman runs in its own terminal and sits there for as long as the
        // user takes to answer it, which is a different wait to describe than
        // a file copy.
        const waitingOnPacman = (command === "install" && previewIsPackage)
                                || (command === "uninstall" && removalIsPackage);
        status = waitingOnPacman ? "Waiting for pacman in its terminal…" : ({inspect: "Reading file…", install: "Copying and installing…", uninstall: "Removing application…", configure: "Saving launch settings…", list: "Refreshing…", reveal: "Opening file manager…", launch: "Starting application…", "check-update": "Checking for updates…", update: "Downloading and installing update…", "check-all-updates": "Checking every application…", "self-check-update": "Checking for a new AppShelf…", "self-update": "Downloading and installing AppShelf…", "tray-start": "Starting the tray…", "tray-stop": "Stopping the tray…", "service-enable": "Enabling tray autostart…", "service-disable": "Disabling tray autostart…", preferences: "Refreshing settings…"})[command] || "Working…";
        backend.write(JSON.stringify(Object.assign({command: command}, values || {})) + "\n");
    }
    function inspect(path, importing) {
        if (path) { importSource = importing ? String(path) : ""; send("inspect", {path: String(path)}); }
    }
    function activateSelected() {
        if (!selected || busy) return;
        if (selected.unmanaged) { inspect(selected.path, true); return; }
        // A package that installed no desktop entry is a library or a command,
        // and there is nothing here to start.
        if (selectedIsPackage && !selected.launchable) {
            failed = true;
            status = selected.name + " installed no application to launch.";
            return;
        }
        send("launch", {id: selected.id});
    }
    function editSelected() {
        if (!selected || selected.unmanaged || busy) return;
        // Environment and isolation are properties of a copy AppShelf launches
        // itself. pacman's files are launched by the desktop, so there is
        // nothing here to set.
        if (selectedIsPackage) { failed = true; status = "Launch settings are for AppImages; " + selected.name + " is a system package."; return; }
        editingId = selected.id;
        settingsEditor.load(selected);
        failed = false;
        activeView = "settings";
    }
    function closeSettings() {
        activeView = "shelf";
        focusList();
    }
    function openPreferences() {
        if (root.modalOpen) return;
        failed = false;
        activeView = "preferences";
    }
    /// Every Settings action is one backend command; remember which so the
    /// buttons can show their own progress instead of a shared spinner.
    function act(command, values) {
        if (!ready || busy) return;
        pendingAction = command;
        send(command, values);
    }
    function saveSettings() {
        if (busy || !ready) return;
        try {
            send("configure", {id: editingId, environment: settingsEditor.environment(), isolation: settingsEditor.isolation});
        } catch (error) {
            failed = true;
            status = String(error);
        }
    }
    function revealSelected() {
        send("reveal", selected ? (selected.unmanaged && !selectedIsPackage ? {path: selected.path} : {id: selected.id}) : {});
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
        if (message.preferences !== undefined) preferences = message.preferences;
        if (message.event === "ready") {
            ready = true;
            status = "Ready";
            focusList();
            const initial = Quickshell.env("APPSHELF_OPEN");
            if (initial) inspect(initial);
            return;
        }
        if (!message.ok) {
            // Clear progress flags first: an early return here used to leave
            // the update spinner running forever on any backend error.
            if (message.command === root.pendingAction) root.pendingAction = "";
            if (message.command === "self-check-update" || message.command === "self-update") { root.selfUpdate = null; root.selfStatusType = "error"; root.selfFeedback = message.error || "AppShelf update failed"; }
            if (message.command === "check-all-updates") { root.allUpdates = null; root.allFeedback = message.error || "Update check failed"; }
            if (message.command === "check-update") { checkingUpdate = false; updateResult = null; updateStatusType = "error"; updateFeedback = message.error || "Update check failed"; }
            if (message.command === "update") { updatingApp = false; updateStatusType = "error"; updateFeedback = message.error || "Update failed"; }
            failed = true;
            status = message.error;
            return;
        }
        if (message.command === root.pendingAction) root.pendingAction = "";
        if (message.command === "self-check-update") {
            root.selfUpdate = message.result;
            root.selfFeedback = message.result.message;
            root.selfStatusType = message.result.has_update ? "available" : (message.result.supported ? "uptodate" : "unsupported");
        }
        if (message.command === "self-update") {
            root.selfUpdate = null;
            root.selfStatusType = "uptodate";
            root.selfFeedback = "AppShelf " + (message.result.version || "") + " installed. Close and reopen AppShelf to run it.";
        }
        if (message.command === "check-all-updates") {
            root.allUpdates = message.result;
            const count = message.result.updates.length;
            root.allFeedback = count === 0
                ? "All " + message.result.checked + " applications are up to date."
                : count + " of " + message.result.checked + " applications have updates.";
        }
        if (message.command === "update" && root.allUpdates) {
            // Drop the row the user just acted on rather than leaving a stale
            // "update available" entry behind.
            root.allUpdates = Object.assign({}, root.allUpdates, {updates: root.allUpdates.updates.filter(u => u.id !== message.result.id)});
        }
        if (message.command === "inspect") { preview = Object.assign(message.result, {external: !!importSource}); installDialog.open(); }
        if (message.command === "install") {
            selectedId = message.result.id;
            installDialog.close();
            preview = null;
        }
        if (message.command === "uninstall") { removeDialog.close(); removal = null; selectedId = ""; }
        if (message.command === "configure") settingsDialog.close();
        if (message.command === "check-update") {
            root.checkingUpdate = false;
            if (message.ok) {
                root.updateResult = message.result;
                root.updateFeedback = message.result.message;
                if (message.result.has_update) {
                    root.updateStatusType = "available";
                } else if (message.result.supported) {
                    root.updateStatusType = "uptodate";
                } else {
                    root.updateStatusType = "unsupported";
                }
            } else {
                root.updateResult = null;
                root.updateStatusType = "error";
                root.updateFeedback = message.error || "Update check failed";
            }
        }
        if (message.command === "update") {
            root.updatingApp = false;
            if (message.ok) {
                root.updateResult = null;
                selectedId = message.result.id;
                root.updateStatusType = "uptodate";
                root.updateFeedback = "Updated " + message.result.name + (message.result.version ? " (" + message.result.version + ")" : "") + "!";
            } else {
                root.updateStatusType = "error";
                root.updateFeedback = message.error || "Update failed";
            }
        }
        status = ({inspect: root.previewIsPackage ? "Review this package before installing" : "Review this AppImage before installing",
                   install: message.result && message.result.kind === "package" ? "Installed by pacman" : "Installed · original download kept",
                   uninstall: "Application removed · personal data kept", configure: "Launch settings saved", launch: "Launch requested", reveal: "Opened in file manager", list: "Shelf refreshed", "check-update": message.result ? message.result.message : "Update check finished", update: "Updated successfully", "check-all-updates": root.allFeedback, "self-check-update": root.selfFeedback, "self-update": root.selfFeedback, "tray-start": "Tray started", "tray-stop": "Tray stopped", "service-enable": "Tray will start with your session", "service-disable": "Tray autostart disabled", preferences: "Settings refreshed"})[message.command] || status || "Ready";
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
                                   font: Theme.family, fontSize: Theme.fontSize, zoom: Theme.zoom, detailsWidth: root.detailsWidth,
                                   detailsVisible: root.detailsVisible, view: root.activeView,
                                   preferences: root.preferences, selfUpdate: root.selfUpdate,
                                   allUpdates: root.allUpdates, pendingAction: root.pendingAction});
        }
        function openPreferences(): string {
            root.openPreferences();
            return root.activeView;
        }
        function closeView(): string {
            root.closeSettings();
            return root.activeView;
        }
        function toggleDetails(): bool {
            root.toggleDetails();
            return root.detailsVisible;
        }
        function setDetailsVisible(v: bool): bool {
            root.detailsVisible = v;
            return root.detailsVisible;
        }
        function zoomIn(): real {
            root.zoomIn();
            return Theme.zoom;
        }
        function zoomOut(): real {
            root.zoomOut();
            return Theme.zoom;
        }
        function resetZoom(): real {
            root.resetZoom();
            return Theme.zoom;
        }
        function setDetailsWidth(w: real): real {
            root.detailsWidth = w;
            return root.detailsWidth;
        }
    }

    FloatingWindow {
        id: window
        title: "AppShelf"
        implicitWidth: 940
        implicitHeight: 600
        minimumSize: Qt.size(480, 340)
        color: Theme.background

        Shortcut { sequence: "Ctrl+O"; enabled: !root.busy && !root.modalOpen; onActivated: picker.open() }
        Shortcut { sequence: "Ctrl+F"; enabled: !root.modalOpen; onActivated: search.forceActiveFocus() }
        Shortcut { sequence: "Ctrl+L"; enabled: !root.modalOpen; onActivated: root.focusList() }
        Shortcut { sequence: "Ctrl+R"; enabled: !root.busy && !root.modalOpen; onActivated: root.send("list") }
        Shortcut { sequence: "Ctrl+E"; enabled: !root.modalOpen; onActivated: root.editSelected() }
        Shortcut { sequence: "Ctrl+U"; enabled: !root.modalOpen && !!root.selected && !root.selected.unmanaged && !root.selectedIsPackage && !root.busy; onActivated: root.send("check-update", {id: root.selected.id}) }
        Shortcut { sequence: "Ctrl+Shift+F"; enabled: !root.modalOpen; onActivated: root.revealSelected() }
        Shortcut { sequence: "F1"; enabled: !root.modalOpen; onActivated: helpDialog.open() }
        Shortcut { sequence: "Ctrl+Return"; enabled: installDialog.opened && !root.busy; onActivated: root.send("install", {path: root.preview.path}) }
        Shortcut { sequence: "Ctrl+S"; enabled: root.activeView === "settings" && !root.busy; onActivated: root.saveSettings() }
        Shortcut { sequence: "Escape"; enabled: root.activeView !== "shelf"; onActivated: root.closeSettings() }
        Shortcut { sequence: "Ctrl+,"; enabled: !root.modalOpen; onActivated: root.activeView === "preferences" ? root.closeSettings() : root.openPreferences() }
        Shortcut { sequence: "Ctrl+B"; enabled: !root.modalOpen; onActivated: root.toggleDetails() }
        Shortcut { sequence: "Ctrl+\\"; enabled: !root.modalOpen; onActivated: root.toggleDetails() }
        Shortcut { sequence: "F4"; enabled: !root.modalOpen; onActivated: root.toggleDetails() }
        Shortcut { sequence: "Ctrl+="; onActivated: root.zoomIn() }
        Shortcut { sequence: "Ctrl++"; onActivated: root.zoomIn() }
        Shortcut { sequence: "Ctrl+Plus"; onActivated: root.zoomIn() }
        Shortcut { sequence: "Ctrl+-"; onActivated: root.zoomOut() }
        Shortcut { sequence: "Ctrl+Minus"; onActivated: root.zoomOut() }
        Shortcut { sequence: "Ctrl+0"; onActivated: root.resetZoom() }
        Shortcut { sequence: "Ctrl+Q"; onActivated: root.quit() }

        WheelHandler {
            acceptedModifiers: Qt.ControlModifier
            onWheel: event => {
                if (event.angleDelta.y > 0) root.zoomIn();
                else if (event.angleDelta.y < 0) root.zoomOut();
            }
        }

        ColumnLayout {
            id: mainLayout
            anchors.fill: parent
            spacing: 0

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: Math.max(48, Math.round(Theme.fontSize * 3.2))
                color: Theme.surface
                clip: true
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 16; anchors.rightMargin: 16
                    spacing: 8

                    ShelfButton {
                        text: "← Back to shelf"
                        visible: root.activeView !== "shelf"
                        onClicked: root.closeSettings()
                    }
                    ShelfText {
                        text: root.activeView === "preferences" ? "AppShelf settings" : ("Launch settings — " + (root.selected ? root.selected.name : ""))
                        font.bold: true
                        font.pixelSize: Theme.fontSize + 2
                        visible: root.activeView !== "shelf"
                    }

                    ShelfText {
                        text: "▤"
                        color: Theme.accent
                        font.pixelSize: Math.min(26, Math.round(Theme.fontSize * 1.8))
                        visible: root.activeView === "shelf"
                    }
                    ShelfText {
                        text: "AppShelf"
                        font.bold: true
                        font.pixelSize: Theme.fontSize + 2
                        visible: root.activeView === "shelf" && (window.width / Theme.zoom) >= 540
                    }
                    Item {
                        Layout.preferredWidth: 6
                        visible: root.activeView === "shelf" && (window.width / Theme.zoom) >= 540
                    }
                    TextField {
                        id: search
                        visible: root.activeView === "shelf"
                        Layout.preferredWidth: Math.min(260, Math.max(110, (window.width / Theme.zoom) * 0.32))
                        implicitHeight: Math.max(30, Math.round(Theme.fontSize * 2.0))
                        placeholderText: (window.width / Theme.zoom) < 720 ? "Search…" : "Search your shelf  /  Ctrl+F"
                        Accessible.name: "Search applications"
                        color: Theme.foreground
                        placeholderTextColor: Theme.secondary
                        selectionColor: Theme.accent
                        selectedTextColor: Theme.background
                        font.family: Theme.family
                        font.pixelSize: Theme.fontSize
                        leftPadding: 8
                        rightPadding: 8
                        background: Rectangle {
                            color: Theme.background
                            border.width: 1
                            border.color: search.activeFocus ? Theme.accent : Theme.line
                            radius: Math.min(Theme.radius, 6)
                        }
                        Keys.onDownPressed: { list.forceActiveFocus(); if (list.count) { list.currentIndex = 0; root.selectedId = root.filtered[0].id; } }
                        Keys.onEscapePressed: { text = ""; list.forceActiveFocus(); }
                    }

                    Item { Layout.fillWidth: true }

                    ShelfButton {
                        text: "?"
                        visible: root.activeView === "shelf"
                        Accessible.name: "Keyboard shortcuts (F1)"
                        onClicked: helpDialog.open()
                    }
                    ShelfButton {
                        // Font Awesome cog from the Nerd Font Omarchy's shell
                        // already draws its own glyphs from, rather than an
                        // emoji the theme font renders however it likes.
                        text: "\uf013"
                        visible: root.activeView === "shelf"
                        primary: root.trayUpdatesPending
                        Accessible.name: "AppShelf settings (Ctrl+,)"
                        onClicked: root.openPreferences()
                    }
                    ShelfButton {
                        text: "Show in Flea"
                        visible: root.activeView === "shelf" && (window.width / Theme.zoom) >= 960 && !root.detailsVisible
                        enabled: root.ready && !root.busy
                        onClicked: root.send("reveal")
                    }
                    ShelfButton {
                        text: (window.width / Theme.zoom) < 640 ? "+" : "+ Install"
                        visible: root.activeView === "shelf"
                        primary: true
                        enabled: root.ready && !root.busy
                        onClicked: picker.open()
                    }
                    ShelfButton {
                        text: root.detailsVisible ? ">>" : "<<"
                        visible: root.activeView === "shelf"
                        Accessible.name: root.detailsVisible ? "Collapse side panel (Ctrl+B)" : "Expand side panel (Ctrl+B)"
                        onClicked: root.toggleDetails()
                    }

                    ShelfButton {
                        text: "Cancel"
                        visible: root.activeView === "settings"
                        enabled: !root.busy
                        onClicked: root.closeSettings()
                    }
                    ShelfButton {
                        text: "Save settings"
                        visible: root.activeView === "settings"
                        primary: true
                        enabled: root.ready && !root.busy
                        onClicked: root.saveSettings()
                    }
                }
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }

            RowLayout {
                visible: root.activeView === "shelf"
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 0
                    RowLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: Theme.scale(root.isNarrow ? 12 : 20)
                    Layout.rightMargin: Theme.scale(root.isNarrow ? 12 : 20)
                    Layout.topMargin: Theme.scale(root.isNarrow ? 10 : 16)
                    Layout.bottomMargin: Theme.scale(root.isNarrow ? 8 : 12)
                    spacing: Theme.scale(root.isNarrow ? 8 : 16)
                    ShelfText { text: "APPLICATIONS"; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                    ShelfText { text: root.shelfCount + " on the shelf · " + root.packageCount + " packages · " + root.discovered.length + " found"; color: Theme.accent; font.pixelSize: Theme.smallSize }
                    Item { Layout.fillWidth: true }
                }

                Item {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    ListView {
                        id: list
                        anchors.fill: parent
                        anchors.margins: Theme.scale(root.isNarrow ? 8 : 12)
                        model: root.filtered
                        clip: true
                        spacing: 4
                        focus: true
                        keyNavigationEnabled: true
                        onCurrentIndexChanged: if (currentIndex >= 0 && currentIndex < root.filtered.length) { root.selectedId = root.filtered[currentIndex].id; root.updateResult = null; root.checkingUpdate = false; root.updatingApp = false; root.updateFeedback = ""; }
                        Keys.onReturnPressed: root.activateSelected()
                        Keys.onDeletePressed: root.removeSelected()
                        Keys.onPressed: event => {
                            if (event.key === Qt.Key_J) { currentIndex = Math.min(count - 1, currentIndex + 1); event.accepted = true; }
                            else if (event.key === Qt.Key_K) { currentIndex = Math.max(0, currentIndex - 1); event.accepted = true; }
                            else if (event.key === Qt.Key_Slash) { search.forceActiveFocus(); event.accepted = true; }
                            else if (event.key === Qt.Key_Escape) { search.text = ""; event.accepted = true; }
                        }
                        ScrollBar.vertical: ScrollBar {}
                        delegate: Item {
                            id: appRow
                            required property var modelData
                            required property int index
                            // The shelf lists what it manages, then what
                            // pacman installed for it, then what it merely
                            // found on disk. Rule off where each begins.
                            readonly property string section: modelData.unmanaged ? "found" : (modelData.kind === "package" ? "package" : "shelf")
                            readonly property bool sectionStart: section !== "shelf"
                                                                && (index === 0 || (root.filtered[index - 1].unmanaged ? "found" : (root.filtered[index - 1].kind === "package" ? "package" : "shelf")) !== section)
                            readonly property string sectionLabel: section === "package" ? "INSTALLED ON YOUR SYSTEM" : "FOUND ON YOUR COMPUTER"
                            width: list.width
                            height: (sectionStart ? sectionHeader.implicitHeight : 0) + rowBody.height

                            Column {
                                id: sectionHeader
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.top: parent.top
                                visible: appRow.sectionStart
                                topPadding: Theme.scale(14)
                                bottomPadding: Theme.scale(6)
                                spacing: Theme.scale(8)
                                Rectangle { width: sectionHeader.width; height: 1; color: Theme.line }
                                ShelfText { text: appRow.sectionLabel; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                            }

                            Rectangle {
                                id: rowBody
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                height: Theme.rowHeight + Theme.scale(12)
                                Accessible.role: Accessible.ListItem
                                Accessible.name: appRow.modelData.name + (appRow.modelData.unmanaged ? ", discovered, Enter to add" : (appRow.modelData.kind === "package" ? ", system package, Enter to launch" : ", managed, Enter to launch"))
                                color: root.selectedId === appRow.modelData.id ? Qt.alpha(Theme.accent, 0.12) : (mouse.containsMouse ? Theme.surface : "transparent")
                                border.width: root.selectedId === appRow.modelData.id ? 1 : 0
                                border.color: Qt.alpha(Theme.accent, 0.5)
                                RowLayout {
                                    anchors.fill: parent
                                    anchors.margins: Theme.scale(root.isNarrow ? 8 : 12)
                                    spacing: Theme.scale(root.isNarrow ? 10 : 14)
                                    Rectangle {
                                        width: Theme.scale(root.isNarrow ? 32 : 36); height: Theme.scale(root.isNarrow ? 32 : 36); color: Theme.surface
                                        Image { id: icon; anchors.fill: parent; source: appRow.modelData.icon; fillMode: Image.PreserveAspectFit; sourceSize: Qt.size(Theme.scale(36), Theme.scale(36)) }
                                        ShelfText { anchors.centerIn: parent; visible: icon.status !== Image.Ready; text: appRow.modelData.kind === "package" ? "\uf187" : appRow.modelData.name.charAt(0).toUpperCase(); color: Theme.accent; font.pixelSize: Theme.scale(root.isNarrow ? 16 : 18) }
                                    }
                                    ColumnLayout {
                                        Layout.fillWidth: true; spacing: Theme.scale(3)
                                        ShelfText { text: appRow.modelData.name; Layout.fillWidth: true; elide: Text.ElideRight; font.bold: true }
                                        // The section header already says these
                                        // were found rather than installed.
                                        ShelfText { text: appRow.modelData.missing ? "File missing" : (appRow.modelData.kind === "package" ? appRow.modelData.version + "  ·  " + appRow.modelData.format : appRow.modelData.format + "  ·  " + root.size(appRow.modelData.size)); Layout.fillWidth: true; elide: Text.ElideRight; color: appRow.modelData.missing ? Theme.danger : Theme.secondary; font.pixelSize: Theme.smallSize }
                                    }
                                    ShelfText { text: "↗"; color: Theme.secondary; visible: !root.isCompact }
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
                    }
                    ColumnLayout {
                        anchors.centerIn: parent
                        width: Math.min(parent.width - Theme.scale(30), Theme.scale(360))
                        visible: root.filtered.length === 0
                        spacing: Theme.scale(root.isNarrow ? 12 : 18)
                        ShelfText { Layout.alignment: Qt.AlignHCenter; text: "▤"; color: Theme.accent; font.pixelSize: Theme.scale(root.isNarrow ? 36 : 48) }
                        ShelfText { Layout.alignment: Qt.AlignHCenter; text: search.text ? "No matching applications" : "A place for your applications"; font.pixelSize: Theme.fontSize + Theme.scale(3); font.bold: true }
                        ShelfText { Layout.fillWidth: true; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.WordWrap; text: search.text ? "Try another name." : "Drop an AppImage or a package here,\nchoose a file, or open one from Flea."; color: Theme.secondary; lineHeight: 1.5 }
                        ShelfButton { Layout.alignment: Qt.AlignHCenter; visible: !search.text; text: "Choose a file"; enabled: root.ready && !root.busy; onClicked: picker.open() }
                    }
                    DropArea {
                        anchors.fill: parent
                        enabled: root.ready && !root.busy && !root.modalOpen
                        onDropped: drop => {
                            if (drop.hasUrls && drop.urls.length === 1) { root.inspect(drop.urls[0]); drop.acceptProposedAction(); }
                            else { root.failed = true; root.status = "Drop one local file at a time."; }
                        }
                    }
                }

            }
            Item {
                id: splitter
                visible: root.detailsVisible
                Layout.fillHeight: true
                Layout.preferredWidth: 9
                    Rectangle {
                        anchors.centerIn: parent
                        width: 1
                        height: parent.height
                        color: splitterMouse.containsMouse || splitterMouse.dragging ? Theme.accent : Theme.line
                    }
                    MouseArea {
                        id: splitterMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.SplitHCursor
                        preventStealing: true
                        property bool dragging: false
                        property real startGlobalX: 0
                        property real startWidth: 0

                        onPressed: mouse => {
                            dragging = true;
                            startGlobalX = mapToItem(mainLayout, mouse.x, 0).x;
                            startWidth = root.detailsWidth * Theme.zoom;
                        }
                        onReleased: dragging = false
                        onCanceled: dragging = false
                        onPositionChanged: mouse => {
                            if (dragging) {
                                const currentGlobalX = mapToItem(mainLayout, mouse.x, 0).x;
                                const delta = currentGlobalX - startGlobalX;
                                const minW = Theme.scale(180);
                                const maxW = Math.max(minW, window.width - Theme.scale(260));
                                const newW = Math.max(minW, Math.min(maxW, startWidth - delta));
                                root.detailsWidth = Math.round(newW / Theme.zoom);
                            }
                        }
                        onDoubleClicked: root.detailsWidth = 270
                    }
                }
            Rectangle {
                id: sidePanel
                visible: root.detailsVisible
                Layout.preferredWidth: Math.min(Math.round(root.detailsWidth * Theme.zoom), Math.round(window.width * (root.isNarrow ? 0.42 : 0.36)))
                Layout.fillHeight: true
                color: Theme.surface
                clip: true
                    ScrollView {
                        anchors.fill: parent
                        contentWidth: availableWidth
                        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                        Item {
                            width: parent.width
                            implicitHeight: detailsColumn.implicitHeight + Theme.scale(44)
                            ColumnLayout {
                                id: detailsColumn
                                anchors.fill: parent
                                anchors.margins: Theme.scale(22)
                                spacing: Theme.scale(16)
                                ShelfText {
                                    Layout.fillWidth: true
                                    text: root.selected ? (root.selected.unmanaged ? "FOUND ON YOUR COMPUTER" : (root.selectedIsPackage ? "INSTALLED ON YOUR SYSTEM" : "ON YOUR SHELF")) : "LOCAL. SIMPLE. YOURS."
                                    color: Theme.secondary
                                    font.pixelSize: Theme.smallSize
                                }
                                ShelfText { Layout.fillWidth: true; text: root.selected ? root.selected.name : "Ready when you are."; font.pixelSize: Theme.fontSize + Theme.scale(6); font.bold: true; wrapMode: Text.Wrap }
                                ShelfText {
                                    Layout.fillWidth: true
                                    wrapMode: Text.WordWrap
                                    lineHeight: 1.5
                                    color: Theme.secondary
                                    text: !root.selected
                                          ? "Keep your applications together and find them in your launcher.\n\nAppImages, and Arch, Debian and RPM packages."
                                          : root.selectedIsPackage
                                            // pacman owns these files, so the panel reports what
                                            // pacman knows rather than anything AppShelf stores.
                                            ? (root.selected.version + "  ·  " + root.selected.format
                                               + (root.selected.description ? "\n\n" + root.selected.description : "")
                                               + "\n\nAdded " + new Date(root.selected.installed * 1000).toLocaleDateString()
                                               + "\nInstalled system-wide by pacman"
                                               + (root.selected.source ? "\nFrom " + root.selected.source : "")
                                               + (root.selected.launchable ? "" : "\n\nNo desktop launcher; this package is a library or a command."))
                                            : root.selected.unmanaged
                                              ? root.selected.path + "\n\nAdd a managed copy to use AppShelf launch settings. The existing installation is kept."
                                              : root.size(root.selected.size) + "  ·  " + root.selected.format + "\nFUSE-free · uruntime\n\nInstalled " + new Date(root.selected.installed * 1000).toLocaleDateString() + "\nIsolation: " + root.selected.isolation + (root.selected.update_info ? "\nUpdates: embedded (" + root.selected.update_info.split("|")[0] + ")" : "")
                                }
                                ShelfButton { Layout.fillWidth: true; visible: !!root.selected; text: root.selected && root.selected.unmanaged ? "Add to AppShelf  ↵" : "Launch  ↗"; primary: true; enabled: root.ready && !root.busy && !!root.selected && !root.selected.missing && !(root.selectedIsPackage && !root.selected.launchable); onClicked: root.activateSelected() }
                                ShelfButton {
                                    Layout.fillWidth: true
                                    visible: !!root.selected && !root.selected.unmanaged && !root.selectedIsPackage && root.updateResult && root.updateResult.has_update
                                    text: root.updatingApp ? "Downloading & installing… ◌" : ("Install update (" + (root.updateResult ? (root.updateResult.latest_version || "new") : "") + ") ↗")
                                    primary: true
                                    enabled: root.ready && !root.busy && !root.checkingUpdate && !root.updatingApp
                                    onClicked: {
                                        root.updatingApp = true;
                                        root.updateFeedback = "";
                                        root.send("update", {id: root.selected.id});
                                    }
                                }
                                ShelfButton {
                                    Layout.fillWidth: true
                                    visible: !!root.selected && !root.selected.unmanaged && !root.selectedIsPackage && (!root.updateResult || !root.updateResult.has_update)
                                    text: root.checkingUpdate ? "Checking updates… ◌" : (root.selected && root.selected.update_info ? "Check for update (Ctrl+U)" : "Check for update")
                                    enabled: root.ready && !root.busy && !root.checkingUpdate && !root.updatingApp
                                    onClicked: {
                                        root.checkingUpdate = true;
                                        root.updateFeedback = "";
                                        root.send("check-update", {id: root.selected.id});
                                    }
                                }
                                Rectangle {
                                    Layout.fillWidth: true
                                    implicitHeight: feedbackRow.implicitHeight + 16
                                    visible: !!root.selected && !root.selected.unmanaged && !root.selectedIsPackage && (root.updateFeedback !== "" || root.checkingUpdate || root.updatingApp)
                                    color: (root.checkingUpdate || root.updatingApp) ? Qt.alpha(Theme.accent, 0.08) : (root.updateStatusType === "error" ? Qt.alpha(Theme.danger, 0.1) : (root.updateStatusType === "unsupported" ? Qt.alpha(Theme.foreground, 0.04) : Qt.alpha(Theme.accent, 0.12)))
                                    border.width: 1
                                    border.color: (root.checkingUpdate || root.updatingApp) ? Theme.accent : (root.updateStatusType === "error" ? Theme.danger : (root.updateStatusType === "unsupported" ? Theme.line : Theme.accent))
                                    radius: Math.min(Theme.radius, 6)
                                    RowLayout {
                                        id: feedbackRow
                                        anchors.fill: parent
                                        anchors.margins: 10
                                        spacing: 10
                                        Item {
                                            width: 16; height: 16
                                            visible: root.checkingUpdate || root.updatingApp
                                            ShelfText {
                                                anchors.centerIn: parent
                                                text: "◌"
                                                color: Theme.accent
                                                font.pixelSize: 14
                                                RotationAnimator on rotation {
                                                    from: 0; to: 360; duration: 900; loops: Animation.Infinite; running: root.checkingUpdate || root.updatingApp
                                                }
                                            }
                                        }
                                        ShelfText {
                                            text: root.updateStatusType === "error" ? "!" : (root.updateStatusType === "unsupported" ? "ℹ" : "✓")
                                            color: root.updateStatusType === "error" ? Theme.danger : (root.updateStatusType === "unsupported" ? Theme.secondary : Theme.accent)
                                            font.bold: true
                                            visible: !root.checkingUpdate && !root.updatingApp
                                        }
                                        ShelfText {
                                            Layout.fillWidth: true
                                            wrapMode: Text.WordWrap
                                            font.pixelSize: Theme.smallSize
                                            text: root.checkingUpdate ? "Connecting to update server…" : (root.updatingApp ? "Downloading and verifying update…" : root.updateFeedback)
                                            color: (root.checkingUpdate || root.updatingApp) ? Theme.foreground : (root.updateStatusType === "error" ? Theme.danger : (root.updateStatusType === "unsupported" ? Theme.secondary : Theme.foreground))
                                        }
                                    }
                                }
                                ShelfButton { Layout.fillWidth: true; visible: !!root.selected; text: "Show in Flea"; enabled: root.ready && !root.busy; onClicked: root.revealSelected() }
                                ShelfButton { Layout.fillWidth: true; visible: !!root.selected && !root.selected.unmanaged && !root.selectedIsPackage; text: "Launch settings…"; enabled: root.ready && !root.busy; onClicked: root.editSelected() }
                                Item { Layout.fillHeight: true }
                                ShelfButton { Layout.fillWidth: true; visible: !!root.selected && !root.selected.unmanaged; text: root.selectedIsPackage ? "Remove package…" : "Uninstall…"; destructive: true; enabled: root.ready && !root.busy; onClicked: root.removeSelected() }
                                ShelfText { Layout.fillWidth: true; text: "↑↓ / j k  Navigate\nEnter     Launch / add\nCtrl+E    Settings\nCtrl+U    Check updates\nCtrl+B    Toggle side panel\nCtrl+,    AppShelf settings\nCtrl + -  Zoom in / out\nCtrl 0    Reset zoom\nF1        All shortcuts"; color: Theme.secondary; font.pixelSize: Theme.smallSize; lineHeight: 1.6 }
                            }
                        }
                    }
                }
            }
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: root.activeView === "settings"

                ScrollView {
                    anchors.fill: parent
                    contentWidth: availableWidth
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                    Item {
                        width: parent.width
                        implicitHeight: settingsPageCol.implicitHeight + 48

                        ColumnLayout {
                            id: settingsPageCol
                            width: Math.min(parent.width - 48, 640)
                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.top: parent.top
                            anchors.topMargin: 24
                            spacing: 16

                            RowLayout {
                                Layout.fillWidth: true
                                ColumnLayout {
                                    spacing: 2
                                    ShelfText {
                                        text: "Launch settings for " + (root.selected ? root.selected.name : "App")
                                        font.bold: true
                                        font.pixelSize: Theme.fontSize + 4
                                    }
                                    ShelfText {
                                        text: "Configure data isolation and environment variables."
                                        color: Theme.secondary
                                        font.pixelSize: Theme.smallSize
                                    }
                                }
                                Item { Layout.fillWidth: true }
                                ShelfButton {
                                    text: "← Back"
                                    onClicked: root.closeSettings()
                                }
                            }

                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }

                            AppSettings {
                                id: settingsEditor
                                Layout.fillWidth: true
                                enabled: !root.busy
                            }

                            ShelfText {
                                Layout.fillWidth: true
                                visible: root.failed
                                text: root.status
                                wrapMode: Text.WordWrap
                                color: Theme.danger
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                Item { Layout.fillWidth: true }
                                ShelfButton {
                                    text: "Cancel"
                                    enabled: !root.busy
                                    onClicked: root.closeSettings()
                                }
                                ShelfButton {
                                    text: "Save settings (Ctrl+S)"
                                    primary: true
                                    enabled: root.ready && !root.busy
                                    onClicked: root.saveSettings()
                                }
                            }
                        }
                    }
                }
            }
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: root.activeView === "preferences"

                ScrollView {
                    anchors.fill: parent
                    contentWidth: availableWidth
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                    Item {
                        width: parent.width
                        implicitHeight: prefsCol.implicitHeight + Theme.scale(48)

                        ColumnLayout {
                            id: prefsCol
                            width: Math.min(parent.width - Theme.scale(48), Theme.scale(640))
                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.top: parent.top
                            anchors.topMargin: Theme.scale(24)
                            spacing: Theme.scale(18)

                            // AppShelf itself — the one application that is not
                            // on the shelf, so this is where it reports its
                            // version and updates itself.
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.scale(14)
                                ShelfText { text: "▤"; color: Theme.accent; font.pixelSize: Theme.scale(34) }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: Theme.scale(3)
                                    ShelfText { text: "AppShelf " + (root.preferences ? root.preferences.version : ""); font.bold: true; font.pixelSize: Theme.fontSize + Theme.scale(6) }
                                    ShelfText {
                                        Layout.fillWidth: true
                                        text: ({appimage: "Running as an AppImage · updates itself in place",
                                                installed: "Installed copy · updates by reinstalling the latest release",
                                                unmanaged: "Built from source or packaged · update it the way you installed it"})[root.selfChannel] || ""
                                        color: Theme.secondary
                                        font.pixelSize: Theme.smallSize
                                        wrapMode: Text.WordWrap
                                    }
                                    ShelfText {
                                        Layout.fillWidth: true
                                        text: "A local application manager for Omarchy: AppImages on its own shelf, and Arch, Debian and RPM packages installed system-wide through pacman."
                                        color: Theme.secondary
                                        font.pixelSize: Theme.smallSize
                                        wrapMode: Text.WordWrap
                                        lineHeight: 1.35
                                    }
                                    ShelfText {
                                        Layout.fillWidth: true
                                        visible: !!root.preferences
                                        text: root.preferences
                                              ? root.preferences.managed + (root.preferences.managed === 1 ? " AppImage on the shelf · " : " AppImages on the shelf · ")
                                                + root.preferences.packages + (root.preferences.packages === 1 ? " package installed" : " packages installed")
                                              : ""
                                        color: Theme.secondary
                                        font.pixelSize: Theme.smallSize
                                    }
                                }
                            }
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }

                            ShelfText { text: "TRAY & BACKGROUND"; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                            ShelfText {
                                Layout.fillWidth: true
                                text: root.trayRunning ? "The tray is running. It checks your applications for updates every four hours."
                                                       : "The tray is not running. Nothing checks for updates in the background."
                                color: root.trayRunning ? Theme.foreground : Theme.secondary
                                wrapMode: Text.WordWrap
                                lineHeight: 1.4
                                font.pixelSize: Theme.smallSize
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.scale(8)
                                ShelfButton {
                                    text: root.pendingAction === "tray-start" ? "Starting… ◌" : "Start tray"
                                    primary: !root.trayRunning
                                    visible: !root.trayRunning
                                    enabled: root.ready && !root.busy
                                    onClicked: root.act("tray-start")
                                }
                                ShelfButton {
                                    text: root.pendingAction === "tray-stop" ? "Stopping… ◌" : "Stop tray"
                                    visible: root.trayRunning
                                    enabled: root.ready && !root.busy
                                    onClicked: root.act("tray-stop")
                                }
                                ShelfButton {
                                    text: root.trayAutostart
                                          ? (root.pendingAction === "service-disable" ? "Disabling… ◌" : "Don't start at login")
                                          : (root.pendingAction === "service-enable" ? "Enabling… ◌" : "Start at login")
                                    enabled: root.ready && !root.busy
                                    onClicked: root.act(root.trayAutostart ? "service-disable" : "service-enable")
                                }
                                Item { Layout.fillWidth: true }
                            }
                            ShelfText {
                                Layout.fillWidth: true
                                text: root.trayAutostart ? "Autostart is on: a systemd user service and desktop entry bring the tray back with your session."
                                                         : "Autostart is off: the tray only runs while AppShelf has started it."
                                color: Theme.secondary
                                wrapMode: Text.WordWrap
                                font.pixelSize: Theme.smallSize
                            }
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }

                            ShelfText { text: "APPSHELF UPDATES"; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.scale(8)
                                ShelfButton {
                                    text: root.pendingAction === "self-check-update" ? "Checking… ◌" : "Check for AppShelf updates"
                                    enabled: root.ready && !root.busy && root.selfUpdatable
                                    onClicked: { root.selfFeedback = ""; root.act("self-check-update"); }
                                }
                                ShelfButton {
                                    text: root.pendingAction === "self-update"
                                          ? "Installing… ◌"
                                          : ("Install " + (root.selfUpdate && root.selfUpdate.latest_version ? root.selfUpdate.latest_version : "update") + "  ↗")
                                    primary: true
                                    visible: !!(root.selfUpdate && root.selfUpdate.has_update)
                                    enabled: root.ready && !root.busy
                                    onClicked: { root.selfFeedback = ""; root.act("self-update"); }
                                }
                                Item { Layout.fillWidth: true }
                            }
                            ShelfText {
                                Layout.fillWidth: true
                                visible: root.selfFeedback !== "" || root.pendingAction === "self-check-update" || root.pendingAction === "self-update"
                                text: root.pendingAction === "self-check-update" ? "Asking GitHub for the latest release…"
                                      : (root.pendingAction === "self-update" ? "Downloading and verifying AppShelf…" : root.selfFeedback)
                                color: root.selfStatusType === "error" ? Theme.danger : (root.selfStatusType === "available" ? Theme.accent : Theme.secondary)
                                wrapMode: Text.WordWrap
                                font.pixelSize: Theme.smallSize
                                lineHeight: 1.4
                            }
                            ShelfText {
                                Layout.fillWidth: true
                                visible: !root.selfUpdatable && !!root.preferences
                                text: "This build cannot update itself."
                                color: Theme.secondary
                                wrapMode: Text.WordWrap
                                font.pixelSize: Theme.smallSize
                            }
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }

                            ShelfText { text: "APPLICATION UPDATES"; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: Theme.scale(8)
                                ShelfButton {
                                    text: root.pendingAction === "check-all-updates" ? "Checking… ◌" : "Check all applications"
                                    enabled: root.ready && !root.busy && root.apps.length > 0
                                    onClicked: { root.allFeedback = ""; root.act("check-all-updates"); }
                                }
                                Item { Layout.fillWidth: true }
                            }
                            ShelfText {
                                Layout.fillWidth: true
                                visible: root.allFeedback !== "" || root.pendingAction === "check-all-updates"
                                text: root.pendingAction === "check-all-updates" ? "Checking every managed application…" : root.allFeedback
                                color: Theme.secondary
                                wrapMode: Text.WordWrap
                                font.pixelSize: Theme.smallSize
                            }
                            Repeater {
                                model: root.allUpdates ? root.allUpdates.updates : []
                                RowLayout {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: Theme.scale(10)
                                    ShelfText {
                                        Layout.fillWidth: true
                                        text: modelData.name + (modelData.latest_version ? "  ·  " + modelData.latest_version : "")
                                        elide: Text.ElideRight
                                    }
                                    ShelfButton {
                                        text: root.pendingAction === "update" ? "Updating… ◌" : "Update ↗"
                                        primary: true
                                        enabled: root.ready && !root.busy
                                        onClicked: root.act("update", {id: modelData.id})
                                    }
                                }
                            }
                            Repeater {
                                model: root.allUpdates ? root.allUpdates.failed : []
                                ShelfText {
                                    required property var modelData
                                    Layout.fillWidth: true
                                    text: modelData.name + ": " + modelData.error
                                    color: Theme.danger
                                    wrapMode: Text.WordWrap
                                    font.pixelSize: Theme.smallSize
                                }
                            }
                            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }

                            ShelfText { text: "LOCATIONS"; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                            ShelfText {
                                Layout.fillWidth: true
                                visible: !!root.preferences
                                text: root.preferences ? ("Command   " + root.preferences.binary + "\nProgram   " + root.preferences.program + "\nData      " + root.preferences.data) : ""
                                color: Theme.secondary
                                wrapMode: Text.WrapAnywhere
                                font.pixelSize: Theme.smallSize
                                lineHeight: 1.5
                            }
                            ShelfText {
                                Layout.fillWidth: true
                                visible: root.failed
                                text: root.status
                                color: Theme.danger
                                wrapMode: Text.WordWrap
                            }
                        }
                    }
                }
            }
            Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }
            RowLayout {
                Layout.fillWidth: true
                implicitHeight: Math.max(26, Math.round(Theme.fontSize * 1.8))
                Layout.leftMargin: 12
                Layout.rightMargin: 12
                spacing: 8
                ShelfText { text: root.busy ? "◌" : (root.failed ? "!" : "●"); color: root.failed ? Theme.danger : Theme.accent }
                ShelfText { Layout.fillWidth: true; text: root.status; color: root.failed ? Theme.danger : Theme.secondary; elide: Text.ElideRight; font.pixelSize: Theme.smallSize }
                ShelfText { text: Math.round(Theme.zoom * 100) + "%"; visible: Theme.zoom !== 1.0; color: Theme.secondary; font.pixelSize: Theme.smallSize }
            }
        }

        FileDialog {
            id: picker
            title: "Choose an application or package"
            nameFilters: ["Applications and packages (*.AppImage *.appimage *.pkg.tar.zst *.pkg.tar.xz *.pkg.tar.gz *.pkg.tar *.deb *.rpm)",
                          "AppImages (*.AppImage *.appimage)",
                          "Arch packages (*.pkg.tar.zst *.pkg.tar.xz *.pkg.tar.gz *.pkg.tar)",
                          "Debian packages (*.deb)",
                          "RPM packages (*.rpm)",
                          "All files (*)"]
            onAccepted: root.inspect(selectedFile)
        }
        Dialog {
            id: installDialog
            anchors.centerIn: parent
            width: Math.min(Theme.scale(510), window.width - 40)
            modal: true
            padding: Theme.scale(24)
            onClosed: root.focusList()
            closePolicy: root.busy ? Popup.NoAutoClose : Popup.CloseOnEscape
            background: Rectangle { color: Theme.background; border.color: Theme.accent; border.width: 1 }
            contentItem: ColumnLayout {
                spacing: Theme.scale(18)
                ShelfText { text: root.previewIsPackage ? "Install system package" : (root.preview && root.preview.external ? "Add existing AppImage" : "Install AppImage"); font.pixelSize: Theme.fontSize + Theme.scale(6); font.bold: true }
                ShelfText { Layout.fillWidth: true; text: root.preview ? (root.previewIsPackage ? root.preview.name + " " + root.preview.version : root.preview.name) : ""; wrapMode: Text.Wrap; color: Theme.accent; font.pixelSize: Theme.fontSize + Theme.scale(2) }
                ShelfText { Layout.fillWidth: true; text: root.preview ? root.preview.path : ""; wrapMode: Text.WrapAnywhere; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                ShelfText {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    lineHeight: 1.4
                    text: root.previewIsPackage
                          // A package does not join the shelf as a copy; it is
                          // installed system-wide, and the terminal that does it
                          // is the part worth saying first.
                          ? (root.preview.arch + " · " + root.preview.format
                             + (root.preview.description ? "\n\n" + root.preview.description : "")
                             + "\n\npacman installs this system-wide. A terminal opens and asks for your password."
                             + (root.preview.installed_version ? "\nReplaces " + root.preview.name + " " + root.preview.installed_version + "." : "")
                             + "\n\nOnly install packages you trust.")
                          : (root.preview ? root.size(root.preview.size) + " · " + root.preview.format + " · FUSE-free\n\n" : "")
                            + (root.preview && root.preview.external ? "Create an AppShelf-managed copy and launcher. The existing installation, launcher, and its settings remain unchanged. You may see both launchers until you remove the old installation with its original manager." : "Add a managed copy to your shelf and create a launcher entry. The original file stays in place.")
                            + "\n\nUse Launch settings to set environment variables and isolation before starting the app. Only install applications you trust."
                }
                Repeater {
                    model: root.previewIsPackage && root.preview.warnings ? root.preview.warnings : []
                    delegate: ShelfText {
                        required property string modelData
                        Layout.fillWidth: true
                        text: "· " + modelData
                        color: Theme.danger
                        wrapMode: Text.WordWrap
                        lineHeight: 1.35
                        font.pixelSize: Theme.smallSize
                    }
                }
                ShelfText {
                    Layout.fillWidth: true
                    visible: root.previewIsPackage && !!root.preview.depends && root.preview.depends.length > 0
                    text: root.previewIsPackage && root.preview.depends ? "Needs: " + root.preview.depends.join(", ") : ""
                    color: Theme.secondary
                    wrapMode: Text.WordWrap
                    lineHeight: 1.35
                    font.pixelSize: Theme.smallSize
                }
                ShelfText { Layout.fillWidth: true; visible: !!root.preview && !!root.preview.note; text: root.preview ? root.preview.note : ""; wrapMode: Text.WordWrap; color: Theme.secondary; font.pixelSize: Theme.smallSize }
                ShelfText { Layout.fillWidth: true; visible: root.failed || root.busy; text: root.status; wrapMode: Text.WordWrap; color: root.failed ? Theme.danger : Theme.accent }
                RowLayout {
                    Layout.fillWidth: true
                    Item { Layout.fillWidth: true }
                    ShelfButton { text: "Cancel"; enabled: !root.busy; onClicked: installDialog.close() }
                    ShelfButton { text: root.busy ? (root.previewIsPackage ? "Waiting for pacman…" : "Installing…") : "Install"; primary: true; enabled: !root.busy && root.ready && !(root.previewIsPackage && root.preview.arch_mismatch); onClicked: root.send("install", {path: root.preview.path}) }
                }
            }
        }
        Dialog {
            id: removeDialog
            onClosed: root.focusList()
            anchors.centerIn: parent
            width: Math.min(Theme.scale(470), window.width - 40)
            modal: true
            padding: Theme.scale(24)
            closePolicy: root.busy ? Popup.NoAutoClose : Popup.CloseOnEscape
            background: Rectangle { color: Theme.background; border.color: Theme.danger; border.width: 1 }
            contentItem: ColumnLayout {
                spacing: Theme.scale(20)
                ShelfText { text: root.removalIsPackage ? "Remove this package from your system?" : "Remove from your shelf?"; font.bold: true; font.pixelSize: Theme.fontSize + Theme.scale(4) }
                ShelfText { Layout.fillWidth: true; text: root.removal ? root.removal.name : ""; wrapMode: Text.Wrap; color: Theme.accent }
                ShelfText { Layout.fillWidth: true; text: root.removalIsPackage
                    ? "pacman removes this package and the dependencies nothing else needs. A terminal opens and asks for your password; nothing is removed until you confirm it there."
                    : "This deletes the managed AppImage copy, its icon and launcher entry. Your original download and personal application data are kept."; wrapMode: Text.WordWrap; lineHeight: 1.5 }
                ShelfText { Layout.fillWidth: true; visible: root.failed; text: root.status; wrapMode: Text.WordWrap; color: Theme.danger }
                RowLayout {
                    Layout.fillWidth: true
                    Item { Layout.fillWidth: true }
                    ShelfButton { text: "Keep app"; enabled: !root.busy; onClicked: removeDialog.close() }
                    ShelfButton { text: root.busy ? (root.removalIsPackage ? "Waiting for pacman…" : "Removing…") : (root.removalIsPackage ? "Remove" : "Uninstall"); destructive: true; enabled: root.ready && !root.busy; onClicked: root.send("uninstall", {id: root.removal.id}) }
                }
            }
        }
        Dialog {
            id: helpDialog
            anchors.centerIn: parent
            width: Math.min(Theme.scale(520), window.width - 40)
            modal: true
            padding: Theme.scale(22)
            onClosed: root.focusList()
            background: Rectangle { color: Theme.background; border.color: Theme.accent; border.width: 1 }
            contentItem: ColumnLayout {
                spacing: Theme.scale(18)
                ShelfText { text: "Your keyboard, your shelf"; font.bold: true; font.pixelSize: Theme.fontSize + Theme.scale(4) }
                ShelfText { Layout.fillWidth: true; text: "AppImages, and Arch (.pkg.tar.zst, .pkg.tar.xz), Debian (.deb) and RPM (.rpm) packages."; color: Theme.secondary; font.pixelSize: Theme.smallSize; wrapMode: Text.WordWrap }
                ShelfText { Layout.fillWidth: true; text: "↑ ↓ / j k      Move through applications\nHome / End     First / last application\nEnter          Launch, or add the selected app\n/ or Ctrl+F    Search\nCtrl+L         Focus application list\nCtrl+O         Choose an AppImage or package\nCtrl+R         Rescan applications\nCtrl+E         Edit launch settings\nCtrl+U         Check for updates\nCtrl+B         Toggle side panel\nCtrl+,         AppShelf settings\nCtrl+Shift+F   Show selected app in Flea\nDelete         Uninstall or remove a package\nCtrl+Enter     Install from preview\nCtrl+S         Save launch settings\nCtrl + / -     Zoom in / out\nCtrl 0         Reset zoom\nTab / Shift+Tab  Move between controls\nSpace / Enter  Activate focused control\nEscape         Close dialog / clear search\nF1             This guide\nCtrl+Q         Quit"; lineHeight: 1.7; font.pixelSize: Theme.fontSize }
                ShelfButton { Layout.alignment: Qt.AlignRight; text: "Back to shelf"; onClicked: helpDialog.close() }
            }
        }
    }
}
