import QtCore
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Effects
import QtWebEngine

ApplicationWindow {
    id: win
    required property var app
    property var cur: null
    property bool sideWanted: true
    readonly property bool showSide: sideWanted && width >= 760
    readonly property bool dark: app.dark
    readonly property color accent: app.theme.accent
    readonly property color solid: app.theme.bg
    readonly property color bg: tint(solid, app.theme.opacity)
    readonly property color sideSolid: app.theme.side
    readonly property color side: tint(sideSolid, app.theme.opacity)
    readonly property color raised: app.theme.raised
    readonly property color fg: app.theme.fg
    readonly property color sub: app.theme.sub
    readonly property color line: app.theme.line
    readonly property color hover: tint(fg, 0.05)
    readonly property color selected: tint(accent, dark ? 0.24 : 0.13)
    readonly property color gold: "#e5a50a"

    function tint(c, a) { return Qt.rgba(c.r, c.g, c.b, a) }
    function esc(s) { return s.replace(/&/g, "&amp;").replace(/</g, "&lt;") }
    function hue(s) {
        let h = 0
        for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 360
        return Qt.hsla(h / 360, dark ? 0.45 : 0.55, dark ? 0.58 : 0.5, 1)
    }
    // YouTube rejects embeds from pages claiming its own origin (error 152)
    function base(link) { return /youtube\.com\//.test(link) ? "https://tidings.lacamar.dev/" : link }
    function openItem(r) {
        if (!r) return
        cur = r
        itemsView.positionViewAtIndex(r.index, ListView.Contain)
        const page = app.article(r.id)
        if (r.web) reader.url = r.link
        else reader.loadHtml(page, base(r.link))
    }
    function step(n) { openItem(app.neighbor(cur ? cur.id : -1, n)) }
    function nextUnread() {
        const r = app.nextUnread(cur ? cur.id : -1)
        if (r) return openItem(r)
        const f = app.feedId > 0 && app.nextFeed()
        if (!f) return toast.say("You're all caught up", false)
        selectFeed(f)
        openItem(app.nextUnread(-1))
    }
    property bool full
    function selectFeed(fid) { app.feedId = fid; cur = null; reader.loadHtml("") }
    function selectFolder(name) { app.folder = name; cur = null; reader.loadHtml("") }
    function shut(name) { return prefs.collapsed.split("\n").indexOf(name) >= 0 }
    function toggleShut(name) {
        const l = prefs.collapsed ? prefs.collapsed.split("\n") : []
        const i = l.indexOf(name)
        i < 0 ? l.push(name) : l.splice(i, 1)
        prefs.collapsed = l.join("\n")
    }
    function present() { show(); raise(); requestActivate() }
    function toggleStar() {
        if (!cur) return
        app.setStarred(cur.id, !cur.starred)
        cur = Object.assign({}, cur, { starred: cur.starred ? 0 : 1 })
    }
    function toggleRead() {
        const r = cur && app.neighbor(cur.id, 0)
        if (r) app.setRead(r.id, !r.read)
    }
    function page(down) {
        if (!cur) return step(1)
        if (cur.web) return
        reader.runJavaScript(`(() => { const e = document.scrollingElement
            if (${down} && e.scrollTop + innerHeight >= e.scrollHeight - 2) return true
            scrollBy({ top: ${down ? 1 : -1} * innerHeight * 0.85, behavior: "smooth" }) })()`,
            end => { if (end) step(1) })
    }
    function scroll(f) { reader.runJavaScript(`scrollBy({ top: ${f} * innerHeight, behavior: "smooth" })`) }
    function applyTheme() {
        if (cur && !cur.web)
            reader.runJavaScript(`document.documentElement.className='${dark ? "dark" : ""}';`
                                 + `document.documentElement.style.setProperty('--accent','${accent}');`
                                 + `document.documentElement.style.setProperty('--fg','${fg}')`)
    }

    width: 1280
    height: 820
    minimumWidth: 640
    minimumHeight: 420
    title: cur ? cur.title + " — Tidings" : "Tidings"
    color: "transparent"
    font.pixelSize: 14
    onClosing: close => { close.accepted = false; hide() }

    palette {
        window: solid; windowText: fg; base: raised; alternateBase: side; text: fg
        button: raised; buttonText: fg; highlight: accent; highlightedText: "white"
        placeholderText: sub; mid: line; light: raised; dark: line
    }

    Settings {
        id: prefs
        property string collapsed
        property int feedId
        property string folder
        property bool unreadOnly
        property bool serif
        property bool calm
        property bool forceDark
        property int expireDays
        property alias width: win.width
        property alias height: win.height
        property alias sideWanted: win.sideWanted
        property alias zoom: reader.zoomFactor
    }

    Component.onCompleted: {
        const p = [prefs.unreadOnly, prefs.folder, prefs.feedId]
        app.setSerif(prefs.serif)
        app.calm = prefs.calm
        app.expireDays = prefs.expireDays
        app.unreadOnly = p[0]
        if (p[1]) app.folder = p[1]
        else app.feedId = p[2]
    }

    Connections {
        target: app
        function onChanged() { prefs.feedId = app.feedId; prefs.folder = app.folder; prefs.unreadOnly = app.unreadOnly }
        function onThemeChanged() { win.applyTheme() }
        function onMessage(t, undoable) { toast.say(t, undoable) }
        function onShown() { win.present() }
        function onSubscribe(u) { win.present(); addDialog.openWith(u, false) }
        function onArticleReady(iid) {
            if (win.cur && win.cur.id === iid && !win.cur.web) reader.loadHtml(app.article(iid), win.base(win.cur.link))
        }
        function onActivate(fid, iid) {
            win.present()
            if (fid < 0) return
            win.selectFeed(fid)
            if (iid) win.openItem(app.neighbor(iid, 0))
        }
    }

    Shortcut { sequences: ["Ctrl+R", "F5"]; onActivated: app.refresh() }
    Shortcut { sequence: "Ctrl+N"; onActivated: addDialog.openWith("", false) }
    Shortcut { sequence: "Ctrl+F"; onActivated: searchBar.show() }
    Shortcut { sequences: ["Shift+J", "L"]; enabled: keys.ok; onActivated: win.selectFeed(app.feedStep(1)) }
    Shortcut { sequences: ["Shift+K", "H"]; enabled: keys.ok; onActivated: win.selectFeed(app.feedStep(-1)) }
    Shortcut { sequences: ["G,G", "Home"]; enabled: keys.ok; onActivated: win.openItem(app.neighbor(-1, 1)) }
    Shortcut { sequences: ["Shift+G", "End"]; enabled: keys.ok; onActivated: win.openItem(app.neighbor(-1, -1)) }
    Shortcut { sequence: "Down"; enabled: keys.ok && !reader.activeFocus; onActivated: win.step(1) }
    Shortcut { sequence: "Up"; enabled: keys.ok && !reader.activeFocus; onActivated: win.step(-1) }
    Shortcut { sequence: "Ctrl+D"; enabled: keys.ok && win.cur; onActivated: win.scroll(0.5) }
    Shortcut { sequence: "Ctrl+U"; enabled: keys.ok && win.cur; onActivated: win.scroll(-0.5) }
    Shortcut { sequence: "R"; enabled: keys.ok; onActivated: app.refresh() }
    Shortcut { sequence: "Shift+A"; enabled: keys.ok; onActivated: app.markAllRead(app.feedId, app.folder) }
    Shortcut {
        sequences: ["Menu", "Shift+F10"]
        enabled: keys.ok && win.cur
        onActivated: itemMenu.openFor(win.cur, itemsView.itemAtIndex(win.cur.index))
    }
    Shortcut { sequence: "Escape"; enabled: keys.ok && searchBar.on && !win.full; onActivated: searchBar.hide() }
    Shortcut { sequence: "/"; enabled: keys.ok; onActivated: searchBar.show() }
    Shortcut { sequence: "F"; enabled: keys.ok && win.cur; onActivated: app.toggleFull(win.cur.id) }
    Shortcut { sequence: "C"; enabled: keys.ok && win.cur; onActivated: app.copy(win.cur.link) }
    Shortcut { sequence: "Escape"; enabled: win.full; onActivated: reader.triggerWebAction(WebEngineView.ExitFullScreen) }
    Shortcut { sequence: "Ctrl+Q"; onActivated: Qt.quit() }
    Shortcut { sequence: "Ctrl+W"; onActivated: win.hide() }
    Shortcut { sequence: "Ctrl+Shift+A"; onActivated: app.markAllRead(app.feedId, app.folder) }
    Shortcut { sequences: ["Ctrl+=", "Ctrl++"]; onActivated: reader.zoomFactor = Math.min(3, reader.zoomFactor + 0.1) }
    Shortcut { sequence: "Ctrl+-"; onActivated: reader.zoomFactor = Math.max(0.5, reader.zoomFactor - 0.1) }
    Shortcut { sequence: "Ctrl+0"; onActivated: reader.zoomFactor = 1 }
    Shortcut { sequence: "J"; enabled: keys.ok; onActivated: win.step(1) }
    Shortcut { sequence: "K"; enabled: keys.ok; onActivated: win.step(-1) }
    Shortcut { sequence: "Space"; enabled: keys.ok; onActivated: win.page(true) }
    Shortcut { sequence: "Shift+Space"; enabled: keys.ok; onActivated: win.page(false) }
    Shortcut { sequence: "N"; enabled: keys.ok; onActivated: win.nextUnread() }
    Shortcut { sequence: "U"; enabled: keys.ok; onActivated: app.unreadOnly = !app.unreadOnly }
    Shortcut { sequence: "M"; enabled: keys.ok; onActivated: win.toggleRead() }
    Shortcut { sequence: "S"; enabled: keys.ok; onActivated: win.toggleStar() }
    Shortcut { sequence: "O"; enabled: keys.ok && win.cur; onActivated: Qt.openUrlExternally(win.cur.link) }
    Shortcut { sequences: ["?", "F1"]; enabled: keys.ok; onActivated: helpDialog.open() }
    QtObject {
        id: keys
        readonly property bool ok: !search.activeFocus && !(win.cur && win.cur.web) && !addDialog.visible
                                   && !renameDialog.visible && !removeDialog.visible && !helpDialog.visible && !folderDialog.visible && !muteDialog.visible
    }

    component IconButton: ToolButton {
        property string tip
        icon.width: 18
        icon.height: 18
        icon.color: checked ? win.accent : win.fg
        implicitWidth: 34
        implicitHeight: 34
        background: Rectangle {
            radius: 8
            color: parent.down || parent.checked ? win.tint(win.fg, 0.1) : parent.hovered ? win.hover : "transparent"
        }
        ToolTip.visible: hovered && tip
        ToolTip.text: tip
        ToolTip.delay: 600
    }

    component Glyph: IconButton {
        enabled: false
        background: null
        implicitWidth: icon.width
        implicitHeight: icon.height
        padding: 0
    }

    component PillButton: Button {
        property bool primary
        property bool danger
        implicitHeight: 36
        leftPadding: 16
        rightPadding: 16
        font.weight: Font.DemiBold
        contentItem: Text {
            text: parent.text
            font: parent.font
            color: parent.primary || parent.danger ? "white" : win.fg
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: 18
            opacity: parent.enabled ? 1 : 0.5
            color: {
                const b = parent.danger ? "#e01b24" : parent.primary ? win.accent : win.tint(win.fg, 0.08)
                return parent.down ? Qt.darker(b, 1.15) : parent.hovered ? Qt.lighter(b, 1.08) : b
            }
        }
    }

    component Field: TextField {
        id: field
        padding: 10
        background: Rectangle {
            radius: 10
            color: win.tint(win.fg, 0.05)
            border.width: field.activeFocus ? 2 : 1
            border.color: field.activeFocus ? win.accent : win.line
        }
    }

    component Rounded: Image {
        property real radius: 10
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        layer.enabled: true
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: mask
            maskThresholdMin: 0.5
            maskSpreadAtMin: 1
        }
        Rectangle { id: mask; width: parent.width; height: parent.height; radius: parent.radius; visible: false; layer.enabled: true }
    }

    component Badge: Rectangle {
        property alias icon: glyph.icon.name
        width: 22
        height: 22
        radius: 7
        Glyph { id: glyph; anchors.centerIn: parent; icon.color: "white"; icon.width: 14; icon.height: 14 }
    }

    component FeedIcon: Item {
        property string name
        property string src
        width: 22
        height: 22
        Rectangle {
            anchors.fill: parent
            radius: 7
            visible: !img.visible
            color: win.hue(parent.name)
            Text {
                anchors.centerIn: parent
                text: parent.parent.name.replace(/^(the|a)\s+/i, "").charAt(0).toUpperCase()
                color: "white"
                font.pixelSize: 12
                font.weight: Font.Bold
            }
        }
        Rounded {
            id: img
            anchors.fill: parent
            radius: 6
            source: parent.src ? "file://" + parent.src : ""
            sourceSize: Qt.size(44, 44)
            visible: status === Image.Ready
        }
    }

    component Pop: Menu {
        padding: 6
        background: Rectangle {
            implicitWidth: 250
            radius: 12
            color: win.raised
            border.color: win.line
        }
    }

    component Entry: MenuItem {
        id: entry
        height: visible ? implicitHeight : 0
        implicitHeight: 34
        contentItem: Label {
            leftPadding: 10
            rightPadding: entry.checkable ? 26 : 10
            text: entry.text
            color: win.fg
            opacity: entry.enabled ? 1 : 0.4
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
        indicator: Label {
            x: entry.width - width - 12
            anchors.verticalCenter: parent.verticalCenter
            text: "✓"
            color: win.accent
            visible: entry.checked
        }
        background: Rectangle { radius: 7; color: entry.highlighted ? win.tint(win.fg, 0.08) : "transparent" }
    }

    component Sep: MenuSeparator {
        height: visible ? implicitHeight : 0
        padding: 5
        contentItem: Rectangle { implicitHeight: 1; color: win.line }
    }

    component Divider: Rectangle {
        width: 1
        Layout.fillHeight: true
        color: win.line
    }

    component Placeholder: Column {
        property alias icon: img.icon.name
        property alias text: label.text
        property alias hint: hintLabel.text
        spacing: 10
        Glyph {
            id: img
            anchors.horizontalCenter: parent.horizontalCenter
            icon.width: 48
            icon.height: 48
            icon.color: win.tint(win.sub, 0.6)
        }
        Label { id: label; anchors.horizontalCenter: parent.horizontalCenter; font.pixelSize: 17; font.weight: Font.DemiBold }
        Label { id: hintLabel; anchors.horizontalCenter: parent.horizontalCenter; color: win.sub; visible: text }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            id: sidebar
            Layout.fillHeight: true
            Layout.preferredWidth: win.showSide && !win.full ? 248 : 0
            visible: Layout.preferredWidth > 0
            clip: true
            color: win.side
            Behavior on Layout.preferredWidth { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }

            ColumnLayout {
                width: 248
                height: parent.height
                spacing: 0

                RowLayout {
                    Layout.preferredHeight: 60
                    Layout.leftMargin: 20
                    Layout.rightMargin: 10
                    spacing: 2
                    Label { text: "Tidings"; font.pixelSize: 18; font.weight: Font.Bold; Layout.fillWidth: true }
                    IconButton {
                        id: refreshBtn
                        tip: "Refresh (Ctrl+R)"
                        icon.name: "view-refresh"
                        enabled: !app.busy
                        onClicked: app.refresh()
                        RotationAnimator on rotation {
                            running: app.busy; from: 0; to: 360; duration: 900; loops: Animation.Infinite
                            onRunningChanged: if (!running) refreshBtn.rotation = 0
                        }
                    }
                    IconButton { tip: "Add Feed (Ctrl+N)"; icon.name: "list-add"; onClicked: addDialog.openWith("", false) }
                    IconButton {
                        id: moreButton
                        tip: "Menu"
                        icon.name: "open-menu-symbolic"
                        onClicked: moreMenu.popup(moreButton, 0, height)
                    }
                }

                ListView {
                    id: feedsView
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    model: app.feedModel
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds
                    ScrollBar.vertical: ScrollBar {}
                    section.property: "folder"
                    section.delegate: Item {
                        id: head
                        required property string section
                        readonly property bool shut: win.shut(section)
                        width: ListView.view.width
                        height: section ? 42 : 0
                        visible: section
                        Rectangle {
                            anchors.fill: parent
                            anchors.leftMargin: 10
                            anchors.rightMargin: 10
                            anchors.topMargin: 10
                            anchors.bottomMargin: 2
                            radius: 8
                            color: app.folder === head.section ? win.selected : headHover.hovered ? win.hover : "transparent"
                            HoverHandler { id: headHover }
                            TapHandler { onTapped: win.selectFolder(head.section) }
                            TapHandler { acceptedButtons: Qt.RightButton; onTapped: folderMenu.openFor(head.section) }
                            RowLayout {
                                anchors.fill: parent
                                anchors.leftMargin: 6
                                anchors.rightMargin: 12
                                spacing: 4
                                IconButton {
                                    icon.name: head.shut ? "go-next-symbolic" : "go-down-symbolic"
                                    icon.color: win.sub
                                    icon.width: 12
                                    icon.height: 12
                                    implicitWidth: 22
                                    implicitHeight: 22
                                    onClicked: win.toggleShut(head.section)
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: head.section
                                    color: win.sub
                                    font.pixelSize: 12
                                    font.weight: Font.DemiBold
                                    elide: Text.ElideRight
                                }
                                Label {
                                    text: app.folders[head.section] || ""
                                    visible: head.shut && text && !app.calm
                                    color: win.sub
                                    font.pixelSize: 12
                                    font.features: { "tnum": 1 }
                                }
                            }
                        }
                    }
                    delegate: Item {
                        id: feedRow
                        required property var model
                        required property int index
                        readonly property bool shut: model.folder && win.shut(model.folder)
                        width: ListView.view.width
                        height: shut ? 0 : index === 1 ? 48 : 38
                        visible: !shut
                        Rectangle {
                            anchors.fill: parent
                            anchors.leftMargin: 10
                            anchors.rightMargin: 10
                            anchors.bottomMargin: feedRow.index === 1 ? 10 : 2
                            radius: 8
                            color: app.feedId === feedRow.model.id && !app.folder ? win.selected : feedHover.hovered ? win.hover : "transparent"
                            HoverHandler { id: feedHover }
                            TapHandler { onTapped: win.selectFeed(feedRow.model.id) }
                            TapHandler {
                                acceptedButtons: Qt.RightButton
                                enabled: feedRow.model.id > 0
                                onTapped: feedMenu.openFor(feedRow.model)
                            }
                            RowLayout {
                                anchors.fill: parent
                                anchors.leftMargin: 10
                                anchors.rightMargin: 12
                                spacing: 10
                                FeedIcon { name: feedRow.model.title; src: feedRow.model.icon; visible: feedRow.model.id > 0 }
                                Badge { visible: feedRow.model.id === 0; color: win.accent; icon: "view-media-playlist" }
                                Badge { visible: feedRow.model.id === -1; color: win.gold; icon: "starred-symbolic" }
                                Label {
                                    Layout.fillWidth: true
                                    text: feedRow.model.title
                                    elide: Text.ElideRight
                                    font.weight: feedRow.model.unread && feedRow.model.id >= 0 ? Font.Medium : Font.Normal
                                }
                                IconButton {
                                    visible: feedRow.model.error
                                    tip: "Couldn't update: " + feedRow.model.error
                                    icon.name: "emblem-warning"
                                    icon.color: win.gold
                                    icon.width: 16
                                    icon.height: 16
                                    implicitWidth: 20
                                    implicitHeight: 20
                                    background: null
                                }
                                Label {
                                    text: feedRow.model.unread
                                    visible: feedRow.model.unread > 0 && !app.calm
                                    color: win.sub
                                    font.pixelSize: 12
                                    font.features: { "tnum": 1 }
                                }
                            }
                        }
                    }
                }
            }
        }

        Divider { visible: sidebar.visible }

        Rectangle {
            visible: !win.full
            Layout.fillHeight: true
            Layout.preferredWidth: Math.max(300, Math.min(420, win.width * 0.32))
            color: win.bg

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                RowLayout {
                    Layout.preferredHeight: 60
                    Layout.leftMargin: 10
                    Layout.rightMargin: 10
                    spacing: 2
                    IconButton {
                        visible: win.width >= 760
                        tip: win.showSide ? "Hide Feeds" : "Show Feeds"
                        icon.name: win.showSide ? "sidebar-collapse-left" : "sidebar-expand-left"
                        icon.color: win.sub
                        onClicked: win.sideWanted = !win.sideWanted
                    }
                    Label {
                        Layout.fillWidth: true
                        Layout.leftMargin: 6
                        text: app.feedTitle
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                    }
                    IconButton {
                        tip: "Search (Ctrl+F)"
                        icon.name: "edit-find"
                        checked: searchBar.on
                        onClicked: searchBar.on ? searchBar.hide() : searchBar.show()
                    }
                    IconButton {
                        tip: "Unread Only (U)"
                        icon.name: "view-filter"
                        checkable: true
                        checked: app.unreadOnly
                        onToggled: app.unreadOnly = checked
                    }
                    IconButton {
                        tip: "Mark All as Read (Ctrl+Shift+A)"
                        icon.name: "mail-mark-read"
                        onClicked: app.markAllRead(app.feedId, app.folder)
                    }
                }

                Item {
                    id: searchBar
                    property bool on
                    function show() { on = true; search.forceActiveFocus(); search.selectAll() }
                    function hide() { on = false; search.text = ""; app.query = ""; reader.findText(""); itemsView.forceActiveFocus() }
                    Layout.fillWidth: true
                    Layout.preferredHeight: on ? 50 : 0
                    clip: true
                    Behavior on Layout.preferredHeight { NumberAnimation { duration: 140; easing.type: Easing.OutCubic } }
                    Field {
                        id: search
                        anchors.fill: parent
                        anchors.leftMargin: 12
                        anchors.rightMargin: 12
                        anchors.bottomMargin: 10
                        leftPadding: 34
                        placeholderText: "Search articles"
                        onTextChanged: searchDelay.restart()
                        Keys.onEscapePressed: searchBar.hide()
                        Keys.onDownPressed: { itemsView.forceActiveFocus(); win.step(1) }
                        Timer { id: searchDelay; interval: 200; onTriggered: app.query = search.text.trim() }
                        Glyph { x: 10; anchors.verticalCenter: parent.verticalCenter; icon.name: "edit-find"; icon.color: win.sub; icon.width: 16; icon.height: 16 }
                    }
                }

                Rectangle { Layout.fillWidth: true; height: 1; color: win.line }

                ListView {
                    id: itemsView
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    model: app.itemModel
                    clip: true
                    bottomMargin: 6
                    spacing: 2
                    boundsBehavior: Flickable.StopAtBounds
                    reuseItems: true
                    onAtYEndChanged: if (atYEnd && count) app.more()
                    ScrollBar.vertical: ScrollBar {}
                    add: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 220 } }
                    displaced: Transition { NumberAnimation { property: "y"; duration: 220; easing.type: Easing.OutCubic } }
                    section.property: "day"
                    section.labelPositioning: ViewSection.InlineLabels | ViewSection.CurrentLabelAtStart
                    section.delegate: Rectangle {
                        required property string section
                        width: ListView.view.width
                        height: 34
                        color: win.bg
                        z: 2
                        Label {
                            x: 28
                            anchors.bottom: parent.bottom
                            anchors.bottomMargin: 6
                            text: parent.section
                            color: win.sub
                            font.pixelSize: 12
                            font.weight: Font.DemiBold
                        }
                    }

                    delegate: Item {
                        id: row
                        required property var model
                        required property int index
                        readonly property bool hasThumb: thumb.status === Image.Ready && thumb.implicitWidth >= 40
                        width: ListView.view.width
                        height: Math.max(text.implicitHeight, hasThumb ? 64 : 0) + 26
                        opacity: row.model.hidden ? 0.45 : 1

                        Rectangle {
                            anchors.fill: parent
                            anchors.leftMargin: 6
                            anchors.rightMargin: 6
                            radius: 10
                            color: win.cur && win.cur.id === row.model.id ? win.selected
                                 : rowHover.hovered ? win.hover : "transparent"
                        }
                        HoverHandler { id: rowHover }
                        TapHandler { onTapped: win.openItem(app.neighbor(row.model.id, 0)) }
                        TapHandler { acceptedButtons: Qt.RightButton; onTapped: itemMenu.openFor(row.model) }

                        Rectangle {
                            x: 14
                            y: 19
                            width: 7
                            height: 7
                            radius: 3.5
                            color: win.accent
                            visible: !row.model.read
                        }

                        Column {
                            id: text
                            x: 28
                            y: 13
                            width: parent.width - x - (row.hasThumb ? 92 : 18)
                            spacing: 3
                            Label {
                                width: parent.width
                                textFormat: Text.StyledText
                                text: (row.model.starred ? `<font color="${win.gold}">★</font>  ` : "")
                                      + win.esc(row.model.feed) + "  ·  " + row.model.when
                                color: win.sub
                                font.pixelSize: 12
                                elide: Text.ElideRight
                            }
                            Label {
                                width: parent.width
                                text: row.model.title
                                color: row.model.read ? win.tint(win.fg, 0.62) : win.fg
                                font.pixelSize: 15
                                font.weight: row.model.read ? Font.Normal : Font.DemiBold
                                wrapMode: Text.Wrap
                                maximumLineCount: 3
                                elide: Text.ElideRight
                                lineHeight: 1.1
                            }
                            Label {
                                width: parent.width
                                text: row.model.preview
                                visible: text
                                color: win.sub
                                font.pixelSize: 13
                                wrapMode: Text.Wrap
                                maximumLineCount: 2
                                elide: Text.ElideRight
                            }
                        }

                        Rounded {
                            id: thumb
                            anchors.right: parent.right
                            anchors.rightMargin: 18
                            y: 14
                            width: 64
                            height: 64
                            source: row.model.image
                            sourceSize: Qt.size(128, 128)
                            visible: row.hasThumb

                            Rectangle {
                                visible: row.model.media !== ""
                                anchors.centerIn: parent
                                width: 26
                                height: 26
                                radius: 13
                                color: "#99000000"

                                Text {
                                    anchors.centerIn: parent
                                    anchors.horizontalCenterOffset: row.model.media === "audio" ? 0 : 1
                                    text: row.model.media === "audio" ? "♪" : "▶"
                                    color: "white"
                                    font.pixelSize: 12
                                }
                            }
                        }
                    }

                    Placeholder {
                        anchors.centerIn: parent
                        visible: itemsView.count === 0 && !app.hiddenCount
                        icon: app.query ? "edit-find" : app.feedId === -1 ? "non-starred-symbolic"
                            : feedsView.count > 2 ? "checkmark" : "application-rss+xml"
                        text: app.query ? "No Results" : app.feedId === -1 ? "Nothing Saved"
                            : feedsView.count > 2 ? "All Caught Up" : "No Feeds"
                        hint: app.query ? "" : app.feedId === -1 ? "Press S to save an article for later"
                            : feedsView.count > 2 ? "" : "Add a website, or drop a link here"
                        PillButton {
                            anchors.horizontalCenter: parent.horizontalCenter
                            visible: feedsView.count <= 2 && !app.query && app.feedId !== -1
                            primary: true
                            text: "Add Feed"
                            onClicked: addDialog.openWith("", false)
                        }
                    }
                }

                Rectangle { Layout.fillWidth: true; height: 1; color: win.line; visible: hiddenBar.visible }
                ItemDelegate {
                    id: hiddenBar
                    Layout.fillWidth: true
                    visible: app.hiddenCount > 0
                    implicitHeight: 38
                    background: Rectangle { color: hiddenBar.hovered ? win.hover : "transparent" }
                    contentItem: Label {
                        text: (app.showHidden ? "Hide " : "Show ") + app.hiddenCount + " duplicate or muted article" + (app.hiddenCount === 1 ? "" : "s")
                        color: win.sub
                        font.pixelSize: 12
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                    onClicked: app.showHidden = !app.showHidden
                }
            }
        }

        Divider { visible: !win.full }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: win.bg

            RowLayout {
                id: readerBar
                width: parent.width
                height: win.full ? 0 : 60
                visible: win.cur && !win.full
                spacing: 2
                Item { Layout.fillWidth: true }
                Rectangle {
                    Layout.rightMargin: 8
                    implicitWidth: modes.implicitWidth + 6
                    implicitHeight: 32
                    radius: 16
                    color: win.tint(win.fg, 0.07)
                    Row {
                        id: modes
                        anchors.centerIn: parent
                        Repeater {
                            model: ["Article", "Web Page"]
                            delegate: Rectangle {
                                required property string modelData
                                required property int index
                                readonly property bool on: win.cur && !!win.cur.web === (index === 1)
                                width: modeLabel.implicitWidth + 26
                                height: 26
                                radius: 13
                                color: on ? win.raised : "transparent"
                                border.color: on && !win.dark ? win.line : "transparent"
                                Label {
                                    id: modeLabel
                                    anchors.centerIn: parent
                                    text: parent.modelData
                                    font.pixelSize: 13
                                    font.weight: parent.on ? Font.DemiBold : Font.Normal
                                    color: parent.on ? win.fg : win.sub
                                }
                                TapHandler {
                                    onTapped: if (!parent.on) {
                                        app.setWeb(win.cur.feed_id, index === 1)
                                        win.openItem(app.neighbor(win.cur.id, 0))
                                    }
                                }
                            }
                        }
                    }
                }
                IconButton {
                    tip: "Full Text On / Off (F)"
                    icon.name: "view-readermode"
                    visible: win.cur && !win.cur.web && win.cur.link && win.cur.media !== "youtube"
                    onClicked: app.toggleFull(win.cur.id)
                }
                IconButton {
                    tip: win.cur && win.cur.starred ? "Remove from Saved (S)" : "Save for Later (S)"
                    icon.name: win.cur && win.cur.starred ? "starred-symbolic" : "non-starred-symbolic"
                    icon.color: win.cur && win.cur.starred ? win.gold : win.fg
                    onClicked: win.toggleStar()
                }
                IconButton {
                    tip: "Open in Browser (O)"
                    icon.name: "external-link-symbolic"
                    Layout.rightMargin: 12
                    onClicked: Qt.openUrlExternally(win.cur.link)
                }
            }

            Rectangle {
                anchors.top: readerBar.bottom
                width: parent.width * reader.loadProgress / 100
                height: 2
                color: win.accent
                visible: reader.loading && win.cur
                z: 1
            }

            WebEngineView {
                id: reader
                anchors.top: readerBar.bottom
                anchors.bottom: parent.bottom
                width: parent.width
                visible: win.cur
                backgroundColor: "transparent"
                onNavigationRequested: req => {
                    if (req.navigationType === WebEngineNavigationRequest.LinkClickedNavigation && !win.cur.web) {
                        Qt.openUrlExternally(req.url)
                        req.reject()
                    }
                }
                onNewWindowRequested: req => Qt.openUrlExternally(req.requestedUrl)
                onLoadingChanged: info => { if (info.status === WebEngineView.LoadSucceededStatus && app.query) findText(app.query) }
                Timer {
                    interval: 5000
                    repeat: true
                    running: win.cur && !win.cur.web && (win.cur.media === "audio" || win.cur.media === "video")
                    onTriggered: {
                        const id = win.cur.id
                        reader.runJavaScript("(m => !m ? -1 : m.ended ? 0 : m.paused ? -1 : m.currentTime)(document.querySelector('audio,video'))",
                                             t => { if (t >= 0) app.setPos(id, t) })
                    }
                }
                Connections {
                    target: reader.profile
                    function onDownloadRequested(d) {
                        d.accept()
                        d.stateChanged.connect(() => {
                            if (d.state === WebEngineDownloadRequest.DownloadCompleted) toast.say(`Saved ${d.downloadFileName} to Downloads`, false)
                            else if (d.state === WebEngineDownloadRequest.DownloadInterrupted) toast.say("Download failed", false)
                        })
                    }
                }
                settings.fullScreenSupportEnabled: true
                settings.forceDarkMode: win.dark && prefs.forceDark
                userScripts.collection: app.cosmetic ? [{ name: "cosmetic", sourceCode: app.cosmetic, injectionPoint: WebEngineScript.DocumentCreation, worldId: WebEngineScript.ApplicationWorld }] : []
                onFullScreenRequested: req => {
                    req.accept()
                    if (req.toggleOn) wasVisibility = win.visibility
                    win.full = req.toggleOn
                    win.visibility = req.toggleOn ? Window.FullScreen : wasVisibility
                }
                property int wasVisibility
                onLinkHovered: url => hoverUrl.text = url
                onContextMenuRequested: req => {
                    req.accepted = true
                    readerMenu.at = { text: req.selectedText, link: req.linkUrl.toString(),
                                      image: req.mediaType === ContextMenuRequest.MediaTypeImage ? req.mediaUrl.toString() : "" }
                    readerMenu.popup()
                }
            }

            Rectangle {
                anchors.left: reader.left
                anchors.bottom: reader.bottom
                anchors.margins: 10
                visible: hoverUrl.text && reader.visible
                width: Math.min(hoverUrl.implicitWidth + 24, reader.width - 20)
                height: 28
                radius: 14
                color: win.raised
                border.color: win.line
                Label {
                    id: hoverUrl
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 12
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideMiddle
                    font.pixelSize: 12
                    color: win.sub
                }
            }

            Placeholder {
                anchors.centerIn: parent
                visible: !win.cur
                icon: "view-readermode"
                text: "No Article Selected"
                hint: "Press Space to start reading, ? for shortcuts"
            }
        }
    }

    Pop {
        id: moreMenu
        Entry { text: "Import Subscriptions…"; onTriggered: app.importOpml() }
        Entry { text: "Export Subscriptions…"; onTriggered: app.exportOpml() }
        Sep {}
        Entry {
            text: "Serif Reading Font"
            checkable: true
            checked: prefs.serif
            onTriggered: { prefs.serif = checked; app.setSerif(checked) }
        }
        Entry {
            text: "Force Dark Web Pages"
            checkable: true
            checked: prefs.forceDark
            onTriggered: prefs.forceDark = checked
        }
        Entry {
            text: "Calm Mode (Hide Counts)"
            checkable: true
            checked: prefs.calm
            onTriggered: { prefs.calm = checked; app.calm = checked }
        }
        Pop {
            title: "Auto-Mark Unread as Read"
            Repeater {
                model: [[0, "Never"], [3, "After 3 Days"], [7, "After a Week"], [14, "After 2 Weeks"], [30, "After a Month"]]
                Entry {
                    required property var modelData
                    text: modelData[1]
                    checkable: true
                    checked: app.expireDays === modelData[0]
                    onTriggered: { prefs.expireDays = modelData[0]; app.expireDays = modelData[0] }
                }
            }
        }
        Entry { text: "Mute Filters…"; onTriggered: muteDialog.openFor("", {}) }
        Sep {}
        Entry {
            text: "Run in Background at Login"
            checkable: true
            checked: app.autostart
            onTriggered: app.autostart = checked
        }
        Entry { text: "Keyboard Shortcuts"; onTriggered: helpDialog.open() }
    }

    Pop {
        id: feedMenu
        property var feed: ({})
        function openFor(f) { feed = { id: f.id, title: f.title, web: f.web, notify: f.notify, url: f.url, site: f.site, folder: f.folder, full: f.full }; popup() }
        Entry { text: "Open Website"; enabled: !!feedMenu.feed.site; onTriggered: Qt.openUrlExternally(feedMenu.feed.site) }
        Entry { text: "Copy Feed Address"; onTriggered: app.copy(feedMenu.feed.url, "Feed address") }
        Sep {}
        Entry { text: "Rename…"; onTriggered: renameDialog.openFor(feedMenu.feed) }
        Entry { text: "Move to Folder…"; onTriggered: folderDialog.openFor(feedMenu.feed, "") }
        Entry {
            text: "Notifications"
            checkable: true
            checked: !!feedMenu.feed.notify
            onTriggered: app.setNotify(feedMenu.feed.id, checked)
        }
        Entry {
            text: "Show Web Page by Default"
            checkable: true
            checked: !!feedMenu.feed.web
            onTriggered: app.setWeb(feedMenu.feed.id, checked)
        }
        Entry {
            text: "Always Load Full Article"
            checkable: true
            checked: !!feedMenu.feed.full
            onTriggered: app.setFull(feedMenu.feed.id, checked)
        }
        Entry { text: "Mute Keyword…"; onTriggered: muteDialog.openFor("", feedMenu.feed) }
        Entry { text: "Mark All as Read"; onTriggered: app.markAllRead(feedMenu.feed.id, "") }
        Sep {}
        Entry { text: "Remove Feed…"; onTriggered: removeDialog.openFor(feedMenu.feed) }
    }

    Pop {
        id: folderMenu
        property string name
        function openFor(n) { name = n; popup() }
        Entry { text: "Mark All as Read"; onTriggered: app.markAllRead(0, folderMenu.name) }
        Entry { text: "Rename…"; onTriggered: folderDialog.openFor({}, folderMenu.name) }
        Entry { text: "Ungroup"; onTriggered: app.renameFolder(folderMenu.name, "") }
    }

    Pop {
        id: readerMenu
        property var at: ({})
        Entry { text: "Copy"; visible: !!readerMenu.at.text; onTriggered: reader.triggerWebAction(WebEngineView.Copy) }
        Entry { text: "Mute “" + (readerMenu.at.text || "").trim().slice(0, 24) + "”…"; visible: !!readerMenu.at.text && readerMenu.at.text.length < 60; onTriggered: muteDialog.openFor(readerMenu.at.text.trim(), {}) }
        Entry { text: "Open Link"; visible: !!readerMenu.at.link; onTriggered: Qt.openUrlExternally(readerMenu.at.link) }
        Entry { text: "Copy Link"; visible: !!readerMenu.at.link; onTriggered: app.copy(readerMenu.at.link) }
        Entry { text: "Open Image"; visible: !!readerMenu.at.image; onTriggered: Qt.openUrlExternally(readerMenu.at.image) }
        Entry { text: "Copy Image"; visible: !!readerMenu.at.image; onTriggered: reader.triggerWebAction(WebEngineView.CopyImageToClipboard) }
        Entry { text: "Save Image"; visible: !!readerMenu.at.image; onTriggered: reader.triggerWebAction(WebEngineView.DownloadImageToDisk) }
        Sep { visible: !!(readerMenu.at.text || readerMenu.at.link || readerMenu.at.image) }
        Entry { text: "Back"; visible: reader.canGoBack && win.cur && win.cur.web; onTriggered: reader.goBack() }
        Entry { text: "Open Article in Browser"; onTriggered: Qt.openUrlExternally(win.cur.link) }
        Entry { text: "Copy Article Link"; onTriggered: app.copy(win.cur.link) }
    }

    Pop {
        id: itemMenu
        property var item: ({})
        function openFor(m, at) { item = { id: m.id, read: m.read, starred: m.starred, link: m.link }; at ? popup(at, 24, at.height) : popup() }
        Entry { text: itemMenu.item.read ? "Mark as Unread" : "Mark as Read"; onTriggered: app.setRead(itemMenu.item.id, !itemMenu.item.read) }
        Entry {
            text: itemMenu.item.starred ? "Remove from Saved" : "Save for Later"
            onTriggered: {
                app.setStarred(itemMenu.item.id, !itemMenu.item.starred)
                if (win.cur && win.cur.id === itemMenu.item.id) win.cur = Object.assign({}, win.cur, { starred: itemMenu.item.starred ? 0 : 1 })
            }
        }
        Entry { text: "Mark Older as Read"; onTriggered: app.markAllRead(app.feedId, app.folder, itemMenu.item.id) }
        Sep {}
        Entry { text: "Open in Browser"; enabled: !!itemMenu.item.link; onTriggered: Qt.openUrlExternally(itemMenu.item.link) }
        Entry { text: "Copy Link"; enabled: !!itemMenu.item.link; onTriggered: app.copy(itemMenu.item.link) }
    }

    component Sheet: Popup {
        anchors.centerIn: Overlay.overlay
        width: Math.min(420, win.width - 48)
        modal: true
        padding: 24
        Overlay.modal: Rectangle { color: win.tint("black", 0.35) }
        background: Rectangle { radius: 16; color: win.raised; border.color: win.dark ? win.line : "transparent" }
        enter: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 140 } }
        exit: Transition { NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 120 } }
    }

    Sheet {
        id: addDialog
        property bool working
        property string error
        property string pre
        property bool go
        function openWith(u, now) { pre = u; go = now; open() }
        function submit() { if (url.text.trim()) { working = true; error = ""; app.addFeed(url.text) } }
        onOpened: {
            error = ""
            working = false
            url.text = pre || app.clipboardUrl()
            url.selectAll()
            url.forceActiveFocus()
            if (go) submit()
        }
        Connections {
            target: app
            function onAdded(err) {
                addDialog.working = false
                addDialog.error = err
                if (!err) addDialog.close()
            }
        }
        ColumnLayout {
            anchors.fill: parent
            spacing: 14
            Label { text: "Add Feed"; font.pixelSize: 18; font.weight: Font.Bold }
            Label { text: "Paste a website or feed address."; color: win.sub }
            Field {
                id: url
                Layout.fillWidth: true
                placeholderText: "example.com"
                enabled: !addDialog.working
                onAccepted: addDialog.submit()
            }
            Label {
                Layout.fillWidth: true
                text: addDialog.error
                visible: text
                color: "#e5484d"
                wrapMode: Text.Wrap
            }
            RowLayout {
                Layout.topMargin: 4
                Item { Layout.fillWidth: true }
                PillButton { text: "Cancel"; onClicked: addDialog.close() }
                PillButton {
                    primary: true
                    text: addDialog.working ? "Looking…" : "Add"
                    enabled: !addDialog.working && url.text.trim()
                    onClicked: addDialog.submit()
                }
            }
        }
    }

    Sheet {
        id: renameDialog
        property var feed: ({})
        function openFor(f) { feed = f; open() }
        function submit() { app.renameFeed(feed.id, name.text); close() }
        onOpened: { name.text = feed.title; name.selectAll(); name.forceActiveFocus() }
        ColumnLayout {
            anchors.fill: parent
            spacing: 14
            Label { text: "Rename Feed"; font.pixelSize: 18; font.weight: Font.Bold }
            Field { id: name; Layout.fillWidth: true; placeholderText: "Use the feed's own name"; onAccepted: renameDialog.submit() }
            RowLayout {
                Layout.topMargin: 4
                Item { Layout.fillWidth: true }
                PillButton { text: "Cancel"; onClicked: renameDialog.close() }
                PillButton { primary: true; text: "Rename"; onClicked: renameDialog.submit() }
            }
        }
    }

    Sheet {
        id: folderDialog
        property var feed: ({})
        property string renaming
        function openFor(f, old) { feed = f; renaming = old; open() }
        function submit(v) {
            if (renaming) app.renameFolder(renaming, v)
            else app.setFolder(feed.id, v)
            close()
        }
        onOpened: { folderName.text = renaming || feed.folder || ""; folderName.selectAll(); folderName.forceActiveFocus() }
        ColumnLayout {
            anchors.fill: parent
            spacing: 14
            Label { text: folderDialog.renaming ? "Rename Folder" : "Move to Folder"; font.pixelSize: 18; font.weight: Font.Bold }
            Field {
                id: folderName
                Layout.fillWidth: true
                placeholderText: folderDialog.renaming ? "Folder name" : "No folder"
                onAccepted: if (text.trim() || !folderDialog.renaming) folderDialog.submit(text)
            }
            Flow {
                Layout.fillWidth: true
                spacing: 8
                visible: !folderDialog.renaming && chips.count > 0
                Repeater {
                    id: chips
                    model: Object.keys(app.folders)
                    delegate: PillButton {
                        required property string modelData
                        implicitHeight: 30
                        text: modelData
                        onClicked: folderDialog.submit(modelData)
                    }
                }
            }
            RowLayout {
                Layout.topMargin: 4
                Item { Layout.fillWidth: true }
                PillButton { text: "Cancel"; onClicked: folderDialog.close() }
                PillButton {
                    primary: true
                    text: folderDialog.renaming ? "Rename" : "Move"
                    enabled: folderName.text.trim() || !folderDialog.renaming
                    onClicked: folderDialog.submit(folderName.text)
                }
            }
        }
    }

    Sheet {
        id: muteDialog
        property var feed: ({})
        property bool titleOnly
        property bool feedOnly
        property int days
        function openFor(t, f) { word.text = t; feed = f; feedOnly = !!f.id; titleOnly = false; days = 0; open() }
        function submit() { app.addMute(word.text, feedOnly ? feed.id : 0, titleOnly, days); word.text = "" }
        onOpened: word.forceActiveFocus()
        ColumnLayout {
            anchors.fill: parent
            spacing: 14
            Label { text: "Mute Filters"; font.pixelSize: 18; font.weight: Font.Bold }
            Label {
                Layout.fillWidth: true
                text: "Hide articles containing a word or phrase. Separate alternatives with commas."
                color: win.sub
                wrapMode: Text.Wrap
            }
            Field { id: word; Layout.fillWidth: true; placeholderText: "e.g. sponsored, giveaway"; onAccepted: muteDialog.submit() }
            Flow {
                Layout.fillWidth: true
                spacing: 8
                PillButton { implicitHeight: 30; text: "Titles Only"; primary: muteDialog.titleOnly; onClicked: muteDialog.titleOnly = !muteDialog.titleOnly }
                PillButton {
                    implicitHeight: 30
                    visible: !!muteDialog.feed.id
                    text: "Only " + muteDialog.feed.title
                    primary: muteDialog.feedOnly
                    onClicked: muteDialog.feedOnly = !muteDialog.feedOnly
                }
                Repeater {
                    model: [[0, "Forever"], [1, "1 Day"], [7, "1 Week"], [30, "1 Month"]]
                    PillButton {
                        required property var modelData
                        implicitHeight: 30
                        text: modelData[1]
                        primary: muteDialog.days === modelData[0]
                        onClicked: muteDialog.days = modelData[0]
                    }
                }
            }
            Repeater {
                model: app.mutes
                RowLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Label { Layout.fillWidth: true; text: modelData.text; elide: Text.ElideRight }
                        Label {
                            Layout.fillWidth: true
                            text: [modelData.feed || "All feeds", modelData.titleOnly ? "titles" : "",
                                   modelData.expires ? "until " + modelData.expires : ""].filter(x => x).join(" · ")
                            color: win.sub
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }
                    }
                    IconButton { tip: "Remove"; icon.name: "edit-delete"; icon.color: win.sub; onClicked: app.removeMute(modelData.id) }
                }
            }
            RowLayout {
                Layout.topMargin: 4
                Item { Layout.fillWidth: true }
                PillButton { text: "Done"; onClicked: muteDialog.close() }
                PillButton { primary: true; text: "Mute"; enabled: word.text.trim(); onClicked: muteDialog.submit() }
            }
        }
    }

    Sheet {
        id: removeDialog
        property var feed: ({})
        function openFor(f) { feed = f; open() }
        ColumnLayout {
            anchors.fill: parent
            spacing: 14
            Label {
                Layout.fillWidth: true
                text: `Remove “${removeDialog.feed.title}”?`
                font.pixelSize: 18
                font.weight: Font.Bold
                wrapMode: Text.Wrap
            }
            Label { text: "Its articles will be deleted too."; color: win.sub }
            RowLayout {
                Layout.topMargin: 4
                Item { Layout.fillWidth: true }
                PillButton { text: "Cancel"; onClicked: removeDialog.close() }
                PillButton {
                    danger: true
                    text: "Remove"
                    onClicked: {
                        if (win.cur && win.cur.feed_id === removeDialog.feed.id) win.cur = null
                        app.removeFeed(removeDialog.feed.id)
                        removeDialog.close()
                    }
                }
            }
        }
    }

    Sheet {
        id: helpDialog
        width: Math.min(680, win.width - 48)
        ColumnLayout {
            anchors.fill: parent
            spacing: 14
            Label { text: "Keyboard Shortcuts"; font.pixelSize: 18; font.weight: Font.Bold }
            GridLayout {
                columns: helpDialog.width > 600 ? 4 : 2
                columnSpacing: 20
                rowSpacing: 8
                Repeater {
                    model: ["J / K  ↓ / ↑", "Next / previous article", "N", "Next unread article", "L / H", "Next / previous feed",
                            "G G / Shift+G", "First / last article", "Space", "Scroll, then next article", "Ctrl+D / U", "Scroll half a page", "U", "Show unread only", "S", "Save for later", "M", "Mark read / unread", "O", "Open in browser",
                            "F", "Full text on / off", "C", "Copy link", "/", "Search", "Ctrl+N", "Add feed", "R", "Refresh",
                            "Shift+A", "Mark all as read", "Menu", "Article actions", "Ctrl+= / −", "Zoom article"]
                    delegate: Label {
                        required property var modelData
                        required property int index
                        text: modelData
                        color: index % 2 ? win.fg : win.sub
                        font.weight: index % 2 ? Font.Normal : Font.DemiBold
                    }
                }
            }
            PillButton { Layout.alignment: Qt.AlignRight; Layout.topMargin: 4; text: "Close"; onClicked: helpDialog.close() }
        }
    }

    Rectangle {
        id: toast
        property bool undoable
        function say(t, u) { label.text = t; undoable = u; opacity = 1; hideTimer.restart() }
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 24
        width: toastRow.implicitWidth + 36
        height: 40
        radius: 20
        color: win.dark ? "#3a3a40" : "#26262b"
        opacity: 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 180 } }
        HoverHandler { id: toastHover }
        Row {
            id: toastRow
            anchors.centerIn: parent
            spacing: 18
            Label { id: label; color: "white" }
            Label {
                visible: toast.undoable
                text: "Undo"
                color: Qt.lighter(win.accent, 1.3)
                font.weight: Font.DemiBold
                TapHandler { onTapped: { app.undo(); toast.opacity = 0 } }
            }
        }
        Timer { id: hideTimer; interval: toast.undoable ? 6000 : 3000; running: !toastHover.hovered && toast.opacity > 0; onTriggered: toast.opacity = 0 }
    }

    DropArea {
        id: drop
        anchors.fill: parent
        z: 10
        keys: ["text/uri-list", "text/plain"]
        onDropped: d => {
            const u = d.hasUrls ? d.urls[0].toString() : d.text.trim()
            if (u.startsWith("file://")) app.importFile(decodeURIComponent(u.slice(7)))
            else addDialog.openWith(u, true)
            d.accept(Qt.CopyAction)
        }
        Rectangle {
            anchors.fill: parent
            anchors.margins: 8
            radius: 16
            visible: drop.containsDrag
            color: win.tint(win.accent, 0.12)
            border.color: win.accent
            border.width: 2
            Label {
                anchors.centerIn: parent
                text: "Drop a link to subscribe, or an OPML file to import"
                font.pixelSize: 16
                font.weight: Font.DemiBold
                color: win.accent
            }
        }
    }
}
