/*
    Gosh MIDI Player — Qt6/Kirigami
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors

    Based on dmidiplayer (Drumstick MIDI File Player) by Pedro López-Cabanillas.
*/

#include "app/settings.hpp"
#include "gui/appconfig.hpp"
#include "gui/playercontroller.hpp"

#include <KAboutData>
#include <KDBusService>
#include <KLocalizedQmlContext>
#include <KLocalizedString>

#include <QApplication>
#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QDir>
#include <QIcon>
#include <QQmlApplicationEngine>
#include <QQuickStyle>
#include <QUrl>
#include <QWindow>

#include <algorithm>

using namespace Qt::Literals::StringLiterals;

namespace {

constexpr auto kAppId = "com.goshapps.GoshMIDI";

// Positional arguments arrive as paths, "file:" URLs or remote URLs. Anything a
// KIO worker can reach is handed on untouched; the resolver deals with it.
QStringList locatorsFrom(const QStringList& arguments, const QString& workingDirectory)
{
    QStringList locators;
    const QDir base(workingDirectory.isEmpty() ? QDir::currentPath() : workingDirectory);
    for (const QString& argument : arguments) {
        if (argument.isEmpty())
            continue;
        const QUrl url = QUrl::fromUserInput(argument, base.absolutePath(), QUrl::AssumeLocalFile);
        const QString locator = dmidi::RemoteFileResolver::locatorFromUrl(url);
        if (!locator.isEmpty())
            locators << locator;
    }
    return locators;
}

} // namespace

int main(int argc, char** argv)
{
    QApplication app(argc, argv);

    // A QML app has no widget style of its own; org.kde.desktop makes the
    // Qt Quick Controls render with the user's Plasma style, colours and fonts.
    if (QQuickStyle::name().isEmpty())
        QQuickStyle::setStyle(u"org.kde.desktop"_s);

    KLocalizedString::setApplicationDomain(QByteArrayLiteral("goshmidi"));
    QCoreApplication::setOrganizationName(u"Gosh Apps"_s);
    QCoreApplication::setOrganizationDomain(u"goshapps.com"_s);
    QApplication::setWindowIcon(QIcon::fromTheme(QString::fromLatin1(kAppId)));

    KAboutData about(u"dmidiplayer"_s,
                     i18n("Gosh MIDI Player"),
                     QString::fromLatin1(GOSHMIDI_VERSION),
                     i18n("A MIDI and karaoke file player for the Plasma desktop"),
                     KAboutLicense::GPL_V3,
                     i18n("© 2006–2026 Pedro López-Cabanillas and contributors"),
                     i18n("Based on dmidiplayer (Drumstick MIDI File Player) by "
                          "Pedro López-Cabanillas. The command name and settings paths "
                          "remain dmidiplayer."),
                     u"https://github.com/goshitsarch-eng/goshmidi"_s);
    about.addAuthor(i18n("Pedro López-Cabanillas"), i18n("Original dmidiplayer"));
    about.addAuthor(i18n("Gosh Apps contributors"), i18n("Qt 6 and Kirigami interface"));
    about.addCredit(i18n("dmidiplayer"), i18n("The original application"), QString(),
                    u"https://dmidiplayer.sourceforge.io/"_s);
    about.setDesktopFileName(QString::fromLatin1(kAppId));
    about.setBugAddress(QByteArrayLiteral("https://github.com/goshitsarch-eng/goshmidi/issues"));
    KAboutData::setApplicationData(about);

    QCommandLineParser parser;
    about.setupCommandLine(&parser);
    parser.setApplicationDescription(
        i18n("Plays MIDI (.mid, .midi), karaoke (.kar), RIFF MIDI (.rmi) and Cakewalk (.wrk) "
             "files, and playlists (.lst), from local storage or a network share."));
    const QCommandLineOption portableOption({u"p"_s, u"portable"_s},
                                            i18n("Keep settings beside the executable."));
    const QCommandLineOption portableFileOption({u"f"_s, u"file"_s},
                                                i18n("Portable settings file to use."),
                                                i18n("file"));
    const QCommandLineOption driverOption({u"d"_s, u"driver"_s},
                                          i18n("MIDI output backend: ALSA, FluidSynth or Dummy."),
                                          i18n("driver"));
    const QCommandLineOption connectionOption({u"c"_s, u"connection"_s},
                                              i18n("MIDI output port to connect to."),
                                              i18n("port"));
    parser.addOption(portableOption);
    parser.addOption(portableFileOption);
    parser.addOption(driverOption);
    parser.addOption(connectionOption);
    parser.addPositionalArgument(i18n("files"),
                                 i18n("MIDI files or a playlist to open."),
                                 i18n("[files…]"));
    parser.process(app);
    about.processCommandLine(&parser);

    if (parser.isSet(portableOption) || parser.isSet(portableFileOption))
        dmidi::AppSettings::setPortable(parser.value(portableFileOption).toStdString());
    dmidi::AppSettings::instance().load();

    // Unique: a second launch hands its files to the running window instead of
    // opening another one.
    KDBusService service(KDBusService::Unique);

    auto* controller = dmidi::PlayerController::instance();

    QQmlApplicationEngine engine;
    KLocalization::setupLocalizedContext(&engine);
    engine.loadFromModule("org.goshapps.goshmidi", "Main");
    if (engine.rootObjects().isEmpty())
        return 1;

    if (parser.isSet(driverOption))
        controller->connectOutput(parser.value(driverOption), parser.value(connectionOption));

    const QStringList startupFiles = locatorsFrom(parser.positionalArguments(), QDir::currentPath());
    if (!startupFiles.isEmpty())
        controller->openLocators(startupFiles, true);

    QObject::connect(&service, &KDBusService::activateRequested, &app,
                     [controller, &engine](const QStringList& arguments,
                                           const QString& workingDirectory) {
                         // arguments[0] is the program name; switches are for
                         // the first instance only, so only paths are kept.
                         QStringList candidates = arguments.mid(1);
                         candidates.removeIf([](const QString& argument) {
                             return argument.startsWith(QLatin1Char('-'));
                         });
                         const QStringList locators = locatorsFrom(candidates, workingDirectory);
                         if (!locators.isEmpty())
                             controller->openLocators(locators, true);

                         const auto roots = engine.rootObjects();
                         for (QObject* root : roots) {
                             if (auto* window = qobject_cast<QWindow*>(root)) {
                                 window->show();
                                 window->raise();
                                 window->requestActivate();
                             }
                         }
                     });

    return app.exec();
}
