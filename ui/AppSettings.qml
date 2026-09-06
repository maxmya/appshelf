import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: editor
    property string isolation: "off"
    property alias environmentText: variables.text
    spacing: Theme.scale(12)
    function load(app) {
        isolation = app.isolation || "off";
        environmentText = Object.keys(app.environment || {}).map(key => key + "=" + app.environment[key]).join("\n");
    }
    function environment() {
        const result = {};
        for (const line of environmentText.split("\n")) {
            if (!line.trim()) continue;
            const at = line.indexOf("=");
            if (at < 1) throw new Error("Use NAME=value, one variable per line.");
            const key = line.slice(0, at).trim();
            if (Object.prototype.hasOwnProperty.call(result, key)) throw new Error("Duplicate environment variable: " + key);
            result[key] = line.slice(at + 1);
        }
        return result;
    }
    ShelfText { text: "Data isolation"; font.bold: true }
    RowLayout {
        Layout.fillWidth: true
        spacing: Theme.scale(6)
        ShelfButton { Layout.fillWidth: true; text: "Shared"; primary: editor.isolation === "off"; onClicked: editor.isolation = "off" }
        ShelfButton { Layout.fillWidth: true; text: "Config + data"; primary: editor.isolation === "config"; onClicked: editor.isolation = "config" }
        ShelfButton { Layout.fillWidth: true; text: "Home + config"; primary: editor.isolation === "home"; onClicked: editor.isolation = "home" }
    }
    ShelfText {
        Layout.fillWidth: true
        text: editor.isolation === "off" ? "Use your usual home and application folders." : "Use separate application folders. Existing settings are not migrated; isolated data is kept after uninstall. This is data isolation, not a security sandbox."
        wrapMode: Text.WordWrap
        color: Theme.secondary
        lineHeight: 1.3
        font.pixelSize: Theme.smallSize
    }
    ShelfText { text: "Environment variables"; font.bold: true }
    ScrollView {
        Layout.fillWidth: true
        Layout.preferredHeight: Theme.scale(130)
        TextArea {
            id: variables
            placeholderText: "NAME=value\nOne variable per line; values are literal."
            color: Theme.foreground
            placeholderTextColor: Theme.secondary
            selectionColor: Theme.accent
            selectedTextColor: Theme.background
            font.family: Theme.family
            font.pixelSize: Theme.fontSize
            textFormat: TextEdit.PlainText
            wrapMode: TextEdit.Wrap
            padding: Theme.scale(10)
            background: Rectangle { color: Theme.surface; border.width: 1; border.color: variables.activeFocus ? Theme.accent : Theme.line }
        }
    }
    ShelfText { Layout.fillWidth: true; text: "Applies on the next launch, including from your desktop launcher."; wrapMode: Text.WordWrap; color: Theme.secondary; font.pixelSize: Theme.smallSize }
}
