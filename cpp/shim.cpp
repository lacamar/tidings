#include "shim.h"

#include <QtCore/QDateTime>
#include <QtCore/QFile>
#include <QtCore/QFileInfo>
#include <QtCore/QFileSystemWatcher>
#include <QtCore/QLocale>
#include <QtGui/QClipboard>
#include <QtGui/QIcon>
#include <QtGui/QImage>
#include <QtGui/QStyleHints>
#include <QtQml/QQmlApplicationEngine>
#include <QtQuick/QQuickWindow>
#include <QtWebEngineCore/QWebEngineCookieStore>
#include <QtWebEngineCore/QWebEngineUrlRequestInterceptor>
#include <QtWebEngineQuick/QQuickWebEngineProfile>
#include <QtWebEngineQuick/QtWebEngineQuick>
#include <QtWidgets/QApplication>
#include <QtWidgets/QMenu>
#include <QtWidgets/QSystemTrayIcon>

#include "tidings/src/bridge.cxxqt.h"
#include "tidings/src/shim.cxxqt.h"

namespace {

int argc = 1;
char arg0[] = "tidings";
char* argv[] = { arg0, nullptr };
QQuickWindow* window = nullptr;

QString qs(rust::Str s)
{
  return QString::fromUtf8(s.data(), static_cast<qsizetype>(s.size()));
}

rust::String rs(const QString& s)
{
  const QByteArray b = s.toUtf8();
  return rust::String(b.constData(), static_cast<size_t>(b.size()));
}

class Blocker : public QWebEngineUrlRequestInterceptor
{
public:
  using QWebEngineUrlRequestInterceptor::QWebEngineUrlRequestInterceptor;

  void interceptRequest(QWebEngineUrlRequestInfo& info) override
  {
    info.setHttpHeader("Sec-GPC", "1");
    info.setHttpHeader("DNT", "1");
    QUrl url = info.requestUrl();
    if (info.resourceType() == QWebEngineUrlRequestInfo::ResourceTypeMainFrame) {
      const QByteArray q = url.query(QUrl::FullyEncoded).toUtf8();
      rust::String out;
      if (clean_query(rust::Str(q.constData(), q.size()), out)) {
        url.setQuery(out.empty() ? QString() : QString::fromUtf8(out.data(), out.size()));
        info.redirect(url);
      }
      return;
    }
    const QByteArray host = url.host().toUtf8();
    const QByteArray path = url.path(QUrl::FullyEncoded).toUtf8();
    if (blocks(rust::Str(host.constData(), host.size()), rust::Str(path.constData(), path.size())))
      info.block(true);
  }
};

void watch_theme(App* app)
{
  const QString file = qs(theme_path());
  auto* w = new QFileSystemWatcher({ QFileInfo(file).absolutePath() }, app);
  // generators either replace the file (dir event) or rewrite it in place (file event)
  auto reload = [w, app, file] {
    if (QFile::exists(file) && !w->files().contains(file))
      w->addPath(file);
    app->loadTheme();
  };
  QObject::connect(w, &QFileSystemWatcher::directoryChanged, app, reload);
  QObject::connect(w, &QFileSystemWatcher::fileChanged, app, reload);
  reload();
}

void add_tray(App* app)
{
  if (!QSystemTrayIcon::isSystemTrayAvailable())
    return;
  auto* tray = new QSystemTrayIcon(QIcon::fromTheme(QStringLiteral("application-rss+xml")), app);
  auto* menu = new QMenu;
  menu->addAction(QStringLiteral("Open Tidings"), app, [app] { Q_EMIT app->shown(); });
  menu->addAction(QStringLiteral("Refresh"), app, [app] { app->refresh(); });
  menu->addSeparator();
  menu->addAction(QStringLiteral("Quit"), qApp, &QCoreApplication::quit);
  tray->setContextMenu(menu);
  QObject::connect(tray, &QSystemTrayIcon::activated, app, [app](QSystemTrayIcon::ActivationReason why) {
    if (why == QSystemTrayIcon::Trigger)
      Q_EMIT app->shown();
  });
  QObject::connect(app, &App::changed, tray, [tray, app] { tray->setToolTip(app->trayTip()); });
  tray->setToolTip(app->trayTip());
  tray->show();
}

}

int run(bool visible, rust::Str url)
{
  QtWebEngineQuick::initialize();
  QApplication qapp(argc, argv);
  QApplication::setApplicationName(QStringLiteral("Tidings"));
  QApplication::setOrganizationName(QStringLiteral("tidings"));
  QApplication::setDesktopFileName(QStringLiteral("dev.lacamar.Tidings"));
  QApplication::setQuitOnLastWindowClosed(false);
  if (QIcon::themeName().isEmpty())
    QIcon::setThemeName(QStringLiteral("breeze"));

  auto* app = new App;
  app->setFeedModel(new FeedListModel(app));
  app->setItemModel(new ItemListModel(app));
  watch_theme(app);
  app->start(qs(url));

  QQuickWindow::setDefaultAlphaBuffer(true);
  auto* profile = QQuickWebEngineProfile::defaultProfile();
  profile->setUrlRequestInterceptor(new Blocker(app));
  profile->cookieStore()->setCookieFilter([](const QWebEngineCookieStore::FilterRequest& r) { return !r.thirdParty; });

  QQmlApplicationEngine engine;
  engine.setInitialProperties({ { QStringLiteral("app"), QVariant::fromValue(static_cast<QObject*>(app)) },
                                { QStringLiteral("visible"), visible } });
  engine.load(QUrl(QStringLiteral("qrc:/Main.qml")));
  if (engine.rootObjects().isEmpty())
    return 1;
  window = qobject_cast<QQuickWindow*>(engine.rootObjects().first());
  add_tray(app);
  const int code = QApplication::exec();
  window = nullptr;
  return code;
}

void app_quit()
{
  QMetaObject::invokeMethod(QCoreApplication::instance(), &QCoreApplication::quit, Qt::QueuedConnection);
}

bool window_active()
{
  return window && window->isActive();
}

rust::String clipboard_text()
{
  return rs(QGuiApplication::clipboard()->text());
}

void set_clipboard(rust::Str text)
{
  QGuiApplication::clipboard()->setText(qs(text));
}

rust::String ui_font()
{
  return rs(QGuiApplication::font().family());
}

void set_color_scheme(bool dark)
{
  QGuiApplication::styleHints()->setColorScheme(dark ? Qt::ColorScheme::Dark : Qt::ColorScheme::Light);
}

rust::String fmt_ts(int64_t ts, uint8_t kind)
{
  const QDateTime t = QDateTime::fromSecsSinceEpoch(ts);
  const QLocale l;
  switch (kind) {
    case 0:
      return rs(l.toString(t.time(), QLocale::ShortFormat));
    case 1:
      return rs(l.toString(t.date(), QLocale::ShortFormat));
    case 2:
      return rs(l.toString(t.date(), QLocale::LongFormat));
    default:
      return rs(l.standaloneDayName(t.date().dayOfWeek()));
  }
}

int64_t days_ago(int64_t ts)
{
  return QDateTime::fromSecsSinceEpoch(ts).date().daysTo(QDate::currentDate());
}

bool save_icon(rust::Slice<const uint8_t> data, rust::Str path)
{
  const QImage img = QImage::fromData(QByteArray::fromRawData(reinterpret_cast<const char*>(data.data()), data.size()));
  if (img.width() < 16)
    return false;
  img.scaled(64, 64, Qt::KeepAspectRatio, Qt::SmoothTransformation).save(qs(path), "PNG");
  return true;
}
