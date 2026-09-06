import QtQuick
import QtQuick.Controls

Button {
    id: control
    property bool primary: false
    property bool destructive: false
    implicitHeight: Math.max(32, Theme.fontSize * 2.5)
    implicitWidth: label.implicitWidth + 24
    padding: 10
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    opacity: enabled ? 1 : 0.4
    contentItem: Text {
        id: label
        text: control.text
        font.family: Theme.family
        font.pixelSize: Theme.fontSize
        color: control.primary ? Theme.background : (control.destructive ? Theme.danger : Theme.foreground)
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }
    background: Rectangle {
        color: control.primary ? Theme.accent : (control.hovered ? Qt.alpha(Theme.foreground, 0.09) : "transparent")
        border.width: 1
        border.color: control.activeFocus ? Theme.accent : (control.primary ? Theme.accent : Theme.line)
        radius: Math.min(Theme.radius, 6)
    }
}
