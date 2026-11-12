#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        #[qobject]
        type QAbstractListModel;
    }

    extern "RustQt" {
        #[qobject]
        #[base = QAbstractListModel]
        type FeedListModel = super::FeedRows;

        #[qobject]
        #[base = QAbstractListModel]
        type ItemListModel = super::ItemRows;
    }

    unsafe extern "RustQt" {
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut FeedListModel>,
            tl: &QModelIndex,
            br: &QModelIndex,
            roles: &QVector_i32,
        );
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        fn begin_insert_rows(
            self: Pin<&mut FeedListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endInsertRows"]
        fn end_insert_rows(self: Pin<&mut FeedListModel>);
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        fn begin_remove_rows(
            self: Pin<&mut FeedListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        fn end_remove_rows(self: Pin<&mut FeedListModel>);
        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut FeedListModel>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut FeedListModel>);
        #[inherit]
        fn index(self: &FeedListModel, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;

        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut ItemListModel>,
            tl: &QModelIndex,
            br: &QModelIndex,
            roles: &QVector_i32,
        );
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        fn begin_insert_rows(
            self: Pin<&mut ItemListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endInsertRows"]
        fn end_insert_rows(self: Pin<&mut ItemListModel>);
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        fn begin_remove_rows(
            self: Pin<&mut ItemListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        fn end_remove_rows(self: Pin<&mut ItemListModel>);
        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut ItemListModel>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut ItemListModel>);
        #[inherit]
        fn index(self: &ItemListModel, row: i32, column: i32, parent: &QModelIndex) -> QModelIndex;
    }

    extern "RustQt" {
        #[qinvokable]
        #[cxx_override]
        fn data(self: &FeedListModel, index: &QModelIndex, role: i32) -> QVariant;
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &FeedListModel) -> QHash_i32_QByteArray;
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &FeedListModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        fn data(self: &ItemListModel, index: &QModelIndex, role: i32) -> QVariant;
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &ItemListModel) -> QHash_i32_QByteArray;
        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &ItemListModel, parent: &QModelIndex) -> i32;
    }

    extern "RustQt" {
        #[qobject]
        #[qproperty(*mut FeedListModel, feed_model, cxx_name = "feedModel")]
        #[qproperty(*mut ItemListModel, item_model, cxx_name = "itemModel")]
        #[qproperty(bool, dark, READ = get_dark, NOTIFY = theme_changed)]
        #[qproperty(QVariant, theme, READ = get_theme, NOTIFY = theme_changed)]
        #[qproperty(bool, busy, READ = get_busy, NOTIFY = busy_changed)]
        #[qproperty(i32, feed_id, cxx_name = "feedId", READ = get_feed_id, WRITE = set_feed_id, NOTIFY = changed)]
        #[qproperty(bool, unread_only, cxx_name = "unreadOnly", READ = get_unread_only, WRITE = set_unread_only, NOTIFY = changed)]
        #[qproperty(QString, query, READ = get_query, WRITE = set_query, NOTIFY = changed)]
        #[qproperty(QString, folder, READ = get_folder, WRITE = set_folder_scope, NOTIFY = changed)]
        #[qproperty(QVariant, folders, READ = get_folders, NOTIFY = changed)]
        #[qproperty(bool, show_hidden, cxx_name = "showHidden", READ = get_show_hidden, WRITE = set_show_hidden, NOTIFY = changed)]
        #[qproperty(i32, hidden_count, cxx_name = "hiddenCount", READ = get_hidden_count, NOTIFY = changed)]
        #[qproperty(bool, calm, READ = get_calm, WRITE = set_calm, NOTIFY = changed)]
        #[qproperty(i32, expire_days, cxx_name = "expireDays", READ = get_expire_days, WRITE = set_expire_days, NOTIFY = changed)]
        #[qproperty(QVariant, mutes, READ = get_mutes, NOTIFY = mutes_changed)]
        #[qproperty(QString, feed_title, cxx_name = "feedTitle", READ = get_feed_title, NOTIFY = changed)]
        #[qproperty(QString, cosmetic, READ = get_cosmetic, NOTIFY = cosmetic_changed)]
        #[qproperty(bool, autostart, READ = get_autostart, WRITE = set_autostart, NOTIFY = changed)]
        type App = super::Backend;

        #[qsignal]
        fn changed(self: Pin<&mut App>);
        #[qsignal]
        #[cxx_name = "mutesChanged"]
        fn mutes_changed(self: Pin<&mut App>);
        #[qsignal]
        #[cxx_name = "themeChanged"]
        fn theme_changed(self: Pin<&mut App>);
        #[qsignal]
        #[cxx_name = "busyChanged"]
        fn busy_changed(self: Pin<&mut App>);
        #[qsignal]
        #[cxx_name = "cosmeticChanged"]
        fn cosmetic_changed(self: Pin<&mut App>);
        #[qsignal]
        fn message(self: Pin<&mut App>, text: QString, undoable: bool);
        #[qsignal]
        fn added(self: Pin<&mut App>, error: QString);
        #[qsignal]
        fn activate(self: Pin<&mut App>, fid: i32, iid: i32);
        #[qsignal]
        fn shown(self: Pin<&mut App>);
        #[qsignal]
        #[cxx_name = "articleReady"]
        fn article_ready(self: Pin<&mut App>, iid: i32);
        #[qsignal]
        fn subscribe(self: Pin<&mut App>, url: QString);

        #[qinvokable]
        #[cxx_name = "getDark"]
        fn get_dark(self: &App) -> bool;
        #[qinvokable]
        #[cxx_name = "getTheme"]
        fn get_theme(self: &App) -> QVariant;
        #[qinvokable]
        #[cxx_name = "getBusy"]
        fn get_busy(self: &App) -> bool;
        #[qinvokable]
        #[cxx_name = "getFeedId"]
        fn get_feed_id(self: &App) -> i32;
        #[qinvokable]
        #[cxx_name = "setFeedId"]
        fn set_feed_id(self: Pin<&mut App>, fid: i32);
        #[qinvokable]
        #[cxx_name = "getUnreadOnly"]
        fn get_unread_only(self: &App) -> bool;
        #[qinvokable]
        #[cxx_name = "setUnreadOnly"]
        fn set_unread_only(self: Pin<&mut App>, on: bool);
        #[qinvokable]
        #[cxx_name = "getQuery"]
        fn get_query(self: &App) -> QString;
        #[qinvokable]
        #[cxx_name = "setQuery"]
        fn set_query(self: Pin<&mut App>, q: QString);
        #[qinvokable]
        #[cxx_name = "getFolder"]
        fn get_folder(self: &App) -> QString;
        #[qinvokable]
        #[cxx_name = "setFolderScope"]
        fn set_folder_scope(self: Pin<&mut App>, folder: QString);
        #[qinvokable]
        #[cxx_name = "getFolders"]
        fn get_folders(self: &App) -> QVariant;
        #[qinvokable]
        #[cxx_name = "getShowHidden"]
        fn get_show_hidden(self: &App) -> bool;
        #[qinvokable]
        #[cxx_name = "setShowHidden"]
        fn set_show_hidden(self: Pin<&mut App>, on: bool);
        #[qinvokable]
        #[cxx_name = "getHiddenCount"]
        fn get_hidden_count(self: &App) -> i32;
        #[qinvokable]
        #[cxx_name = "getCalm"]
        fn get_calm(self: &App) -> bool;
        #[qinvokable]
        #[cxx_name = "setCalm"]
        fn set_calm(self: Pin<&mut App>, on: bool);
        #[qinvokable]
        #[cxx_name = "getExpireDays"]
        fn get_expire_days(self: &App) -> i32;
        #[qinvokable]
        #[cxx_name = "setExpireDays"]
        fn set_expire_days(self: Pin<&mut App>, days: i32);
        #[qinvokable]
        #[cxx_name = "getMutes"]
        fn get_mutes(self: &App) -> QVariant;
        #[qinvokable]
        #[cxx_name = "getFeedTitle"]
        fn get_feed_title(self: &App) -> QString;
        #[qinvokable]
        #[cxx_name = "getCosmetic"]
        fn get_cosmetic(self: &App) -> QString;
        #[qinvokable]
        #[cxx_name = "getAutostart"]
        fn get_autostart(self: &App) -> bool;
        #[qinvokable]
        #[cxx_name = "setAutostart"]
        fn set_autostart(self: Pin<&mut App>, on: bool);

        #[qinvokable]
        fn start(self: Pin<&mut App>, url: QString);
        #[qinvokable]
        #[cxx_name = "loadTheme"]
        fn load_theme(self: Pin<&mut App>);
        #[qinvokable]
        #[cxx_name = "trayTip"]
        fn tray_tip(self: &App) -> QString;

        #[qinvokable]
        #[cxx_name = "addMute"]
        fn add_mute(self: Pin<&mut App>, text: QString, fid: i32, title_only: bool, days: i32);
        #[qinvokable]
        #[cxx_name = "removeMute"]
        fn remove_mute(self: Pin<&mut App>, mid: i32);
        #[qinvokable]
        fn more(self: Pin<&mut App>);
        #[qinvokable]
        #[cxx_name = "setPos"]
        fn set_pos(self: Pin<&mut App>, iid: i32, t: f64);
        #[qinvokable]
        fn neighbor(self: &App, iid: i32, step: i32) -> QVariant;
        #[qinvokable]
        #[cxx_name = "feedStep"]
        fn feed_step(self: &App, n: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "nextUnread"]
        fn next_unread(self: &App, iid: i32) -> QVariant;
        #[qinvokable]
        #[cxx_name = "nextFeed"]
        fn next_feed(self: &App) -> i32;
        #[qinvokable]
        fn article(self: Pin<&mut App>, iid: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "toggleFull"]
        fn toggle_full(self: Pin<&mut App>, iid: i32);
        #[qinvokable]
        #[cxx_name = "setRead"]
        fn set_read(self: Pin<&mut App>, iid: i32, on: bool);
        #[qinvokable]
        #[cxx_name = "setStarred"]
        fn set_starred(self: Pin<&mut App>, iid: i32, on: bool);
        #[qinvokable]
        #[cxx_name = "setWeb"]
        fn set_web(self: Pin<&mut App>, fid: i32, on: bool);
        #[qinvokable]
        #[cxx_name = "setSerif"]
        fn set_serif(self: Pin<&mut App>, on: bool);
        #[qinvokable]
        #[cxx_name = "setFull"]
        fn set_full(self: Pin<&mut App>, fid: i32, on: bool);
        #[qinvokable]
        #[cxx_name = "setNotify"]
        fn set_notify(self: Pin<&mut App>, fid: i32, on: bool);
        #[qinvokable]
        #[cxx_name = "renameFeed"]
        fn rename_feed(self: Pin<&mut App>, fid: i32, name: QString);
        #[qinvokable]
        #[cxx_name = "setFolder"]
        fn set_folder(self: Pin<&mut App>, fid: i32, name: QString);
        #[qinvokable]
        #[cxx_name = "renameFolder"]
        fn rename_folder(self: Pin<&mut App>, old: QString, to: QString);
        #[qinvokable]
        fn refresh(self: Pin<&mut App>);
        #[qinvokable]
        #[cxx_name = "addFeed"]
        fn add_feed(self: Pin<&mut App>, url: QString);
        #[qinvokable]
        #[cxx_name = "removeFeed"]
        fn remove_feed(self: Pin<&mut App>, fid: i32);
        #[qinvokable]
        #[cxx_name = "markAllRead"]
        fn mark_all_read(self: Pin<&mut App>, fid: i32, folder: QString);
        #[qinvokable]
        #[cxx_name = "markAllRead"]
        fn mark_older_read(self: Pin<&mut App>, fid: i32, folder: QString, older_than: i32);
        #[qinvokable]
        fn undo(self: Pin<&mut App>);
        #[qinvokable]
        #[cxx_name = "clipboardUrl"]
        fn clipboard_url(self: &App) -> QString;
        #[qinvokable]
        fn copy(self: Pin<&mut App>, text: QString);
        #[qinvokable]
        #[cxx_name = "copy"]
        fn copy_what(self: Pin<&mut App>, text: QString, what: QString);
        #[qinvokable]
        #[cxx_name = "importOpml"]
        fn import_opml(self: Pin<&mut App>);
        #[qinvokable]
        #[cxx_name = "importFile"]
        fn import_file(self: Pin<&mut App>, path: QString);
        #[qinvokable]
        #[cxx_name = "exportOpml"]
        fn export_opml(self: Pin<&mut App>);
    }

    impl cxx_qt::Threading for App {}
}

pub use crate::backend::Backend;
pub use crate::models::{FeedRows, ItemRows};
