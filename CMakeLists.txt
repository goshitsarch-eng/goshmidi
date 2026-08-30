#[==========================================================================[
Gosh MIDI Player — Qt 6 / KDE Frameworks 6 (Kirigami)
Based on dmidiplayer (Drumstick MIDI File Player)
Copyright (C) 2006-2026 Pedro Lopez-Cabanillas <plcl@users.sourceforge.net>
#]==========================================================================]

cmake_minimum_required(VERSION 3.20)

project(dmidiplayer
    VERSION 2.0.0
    LANGUAGES C CXX
    DESCRIPTION "Gosh MIDI Player"
    HOMEPAGE_URL "https://github.com/goshitsarch-eng/goshmidi"
)

set(GOSH_APP_ID "com.goshapps.GoshMIDI")

set(QT_MIN_VERSION 6.5.0)
set(KF_MIN_VERSION 6.21.0)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(ECM ${KF_MIN_VERSION} REQUIRED NO_MODULE)
list(APPEND CMAKE_MODULE_PATH ${ECM_MODULE_PATH} "${CMAKE_SOURCE_DIR}/cmake")

include(KDEInstallDirs)
include(KDECMakeSettings)
include(ECMSetupVersion)
include(FeatureSummary)
include(SCMRevision)

option(BUILD_DOCS "Process Markdown sources of man pages and help files" OFF)
option(BUILD_TESTING "Build sequence and playlist tests" ON)

find_package(Qt6 ${QT_MIN_VERSION} REQUIRED COMPONENTS
    Core
    Gui
    Qml
    Quick
    QuickControls2
    Widgets
    Core5Compat
)
find_package(KF6 ${KF_MIN_VERSION} REQUIRED COMPONENTS
    CoreAddons
    I18n
    Kirigami
    DBusAddons
    KIO
    IconThemes
    ColorScheme
)

find_package(PkgConfig REQUIRED)
find_package(Threads REQUIRED)
pkg_check_modules(ALSA REQUIRED IMPORTED_TARGET alsa)
pkg_check_modules(FLUIDSYNTH REQUIRED IMPORTED_TARGET fluidsynth)
pkg_check_modules(UCHARDET REQUIRED IMPORTED_TARGET uchardet)

qt_standard_project_setup(REQUIRES 6.5)
set(CMAKE_AUTOMOC ON)

#[[ Toolkit-independent MIDI engine, shared with the headless tests. ]]
add_library(dmidi-core STATIC
    src/midi/events.cpp
    src/midi/sequence.cpp
    src/midi/output.cpp
    src/midi/player.cpp
    src/midi/sysex.cpp
    src/app/dispatch.cpp
    src/app/playlist.cpp
    src/app/settings.cpp
)
target_include_directories(dmidi-core PUBLIC ${CMAKE_SOURCE_DIR}/src)
target_link_libraries(dmidi-core PUBLIC
    Qt6::Core
    Qt6::Core5Compat
    PkgConfig::ALSA
    PkgConfig::FLUIDSYNTH
    PkgConfig::UCHARDET
    Threads::Threads
)
set_target_properties(dmidi-core PROPERTIES POSITION_INDEPENDENT_CODE ON)

qt_add_executable(dmidiplayer src/main.cpp)

qt_add_qml_module(dmidiplayer
    URI org.goshapps.goshmidi
    VERSION 1.0
    DEPENDENCIES
        QtQuick
        org.kde.coreaddons
        org.kde.ki18n
    QML_FILES
        src/qml/Main.qml
        src/qml/PlayerPage.qml
        src/qml/PlaylistPage.qml
        src/qml/LyricsView.qml
        src/qml/ChannelsView.qml
        src/qml/PianoBoard.qml
        src/qml/RhythmLamps.qml
        src/qml/MidiSetupPage.qml
        src/qml/SettingsPage.qml
        src/qml/HelpPage.qml
        src/qml/JumpToBarDialog.qml
        src/qml/LoopDialog.qml
        src/qml/FileInfoDialog.qml
        src/qml/OpenLocationDialog.qml
    SOURCES
        src/gui/appconfig.cpp
        src/gui/appconfig.hpp
        src/gui/channelmodel.cpp
        src/gui/channelmodel.hpp
        src/gui/pianoview.cpp
        src/gui/pianoview.hpp
        src/gui/playercontroller.cpp
        src/gui/playercontroller.hpp
        src/gui/playlistmodel.cpp
        src/gui/playlistmodel.hpp
        src/gui/remotefileresolver.cpp
        src/gui/remotefileresolver.hpp
)

# The generated QML type registrations include the GUI headers by basename.
target_include_directories(dmidiplayer PRIVATE ${CMAKE_SOURCE_DIR}/src/gui)

target_link_libraries(dmidiplayer PRIVATE
    dmidi-core
    Qt6::Core
    Qt6::Gui
    Qt6::Qml
    Qt6::Quick
    Qt6::QuickControls2
    Qt6::Widgets
    KF6::CoreAddons
    KF6::I18n
    KF6::I18nQml
    KF6::Kirigami
    KF6::DBusAddons
    KF6::KIOCore
    KF6::IconThemes
    KF6::ColorScheme
)

target_compile_definitions(dmidiplayer PRIVATE
    GOSHMIDI_VERSION="${PROJECT_VERSION}"
    DATADIR="${KDE_INSTALL_FULL_DATADIR}"
)
if(DEFINED PROJECT_WC_REVISION)
    target_compile_definitions(dmidiplayer PRIVATE REVISION=${PROJECT_WC_REVISION})
endif()

add_subdirectory(examples)
add_subdirectory(icons)

if(BUILD_DOCS)
    add_subdirectory(docs)
endif()

if(BUILD_TESTING)
    enable_testing()
    add_executable(dmidiplayer-test tests/test_sequence.cpp)
    target_link_libraries(dmidiplayer-test PRIVATE dmidi-core)
    add_test(NAME sequence_playlist
             COMMAND dmidiplayer-test ${CMAKE_SOURCE_DIR}/examples)
    add_test(NAME canonical_application_identity
             COMMAND ${CMAKE_COMMAND}
                     -DPROJECT_SOURCE_DIR=${CMAKE_SOURCE_DIR}
                     -P ${CMAKE_SOURCE_DIR}/tests/check_identity.cmake)
    add_test(NAME single_instance_file_open_contract
             COMMAND ${CMAKE_COMMAND}
                     -DPROJECT_SOURCE_DIR=${CMAKE_SOURCE_DIR}
                     -P ${CMAKE_SOURCE_DIR}/tests/check_single_instance_open.cmake)
    add_test(NAME no_legacy_toolkit_remnants
             COMMAND ${CMAKE_COMMAND}
                     -DPROJECT_SOURCE_DIR=${CMAKE_SOURCE_DIR}
                     -P ${CMAKE_SOURCE_DIR}/tests/check_no_gtk.cmake)
endif()

install(TARGETS dmidiplayer ${KDE_INSTALL_TARGETS_DEFAULT_ARGS})
install(FILES ${GOSH_APP_ID}.desktop DESTINATION ${KDE_INSTALL_APPDIR})
install(FILES ${GOSH_APP_ID}.metainfo.xml DESTINATION ${KDE_INSTALL_METAINFODIR})
install(FILES LICENSE DESTINATION ${KDE_INSTALL_DATAROOTDIR}/licenses/${GOSH_APP_ID})

feature_summary(WHAT ALL INCLUDE_QUIET_PACKAGES FATAL_ON_MISSING_REQUIRED_PACKAGES)
