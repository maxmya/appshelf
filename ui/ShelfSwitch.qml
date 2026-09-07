import QtQuick
import QtQuick.Controls

// A setting that is on or off, drawn as a switch rather than a button whose
// label has to describe the state it is not in. The label sits on the left and
// the track on the right, so a column of these reads as a list of settings.
//
// `checked` is expected to be bound to state the backend owns. A click breaks
// that binding, so `onToggled` hands the intent back to the caller and the
// binding is restored there — the backend stays the one deciding what is true.
Switch {
    id: control
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    opacity: enabled ? 1 : 0.45
    padding: 0
    spacing: Theme.scale(12)
    implicitHeight: Math.max(Math.round(Theme.fontSize * 2.0), track.implicitHeight)

    indicator: Rectangle {
        id: track
        implicitWidth: Math.round(Theme.fontSize * 2.7)
        implicitHeight: Math.round(Theme.fontSize * 1.45)
        x: control.width - width
        y: (control.height - height) / 2
        radius: Math.min(Theme.radius, Math.round(height / 2))
        color: control.checked ? Theme.accent : "transparent"
        border.width: 1
        border.color: control.activeFocus
                      ? Theme.accent
                      : (control.checked ? Theme.accent : (control.hovered ? Theme.foreground : Theme.line))
        Behavior on color { ColorAnimation { duration: 120 } }
        Behavior on border.color { ColorAnimation { duration: 120 } }

        Rectangle {
            id: knob
            width: track.height - Theme.scale(6)
            height: width
            radius: Math.min(Theme.radius, Math.round(height / 2))
            y: (track.height - height) / 2
            x: control.checked ? track.width - width - Theme.scale(3) : Theme.scale(3)
            color: control.checked ? Theme.background : Theme.secondary
            Behavior on x { NumberAnimation { duration: 130; easing.type: Easing.OutCubic } }
            Behavior on color { ColorAnimation { duration: 120 } }
        }
    }

    contentItem: Text {
        text: control.text
        font.family: Theme.family
        font.pixelSize: Theme.fontSize
        color: Theme.foreground
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
        rightPadding: control.indicator.width + control.spacing
    }
}
