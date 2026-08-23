#[==========================================================================[
Drumstick MIDI File Player — GTK4/libadwaita
Copyright (C) 2006-2026 Pedro Lopez-Cabanillas <plcl@users.sourceforge.net>
#]==========================================================================]

cmake_minimum_required(VERSION 3.16)

project(dmidiplayer
    VERSION 1.7.5
    LANGUAGES C CXX
    DESCRIPTION "Drumstick MIDI File Player"
    HOMEPAGE_URL "https://sourceforge.net/p/dmidiplayer/"
)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_INCLUDE_CURRENT_DIR ON)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_SOURCE_DIR}/cmake")
include(GNUInstallDirs)
include(SCMRevision)

option(BUILD_DOCS "Process Markdown sources of man pages and help files" OFF)
option(BUILD_TESTING "Build sequence and playlist tests" ON)

find_package(PkgConfig REQUIRED)
find_package(Threads REQUIRED)
pkg_check_modules(GTK4 REQUIRED IMPORTED_TARGET gtk4)
pkg_check_modules(ADWAITA REQUIRED IMPORTED_TARGET libadwaita-1)
pkg_check_modules(ALSA REQUIRED IMPORTED_TARGET alsa)
pkg_check_modules(FLUIDSYNTH REQUIRED IMPORTED_TARGET fluidsynth)
pkg_check_modules(UCHARDET REQUIRED IMPORTED_TARGET uchardet)
pkg_check_modules(GLIB REQUIRED IMPORTED_TARGET glib-2.0)

find_program(GLIB_COMPILE_RESOURCES glib-compile-resources REQUIRED)

set(GRESOURCE_XML ${CMAKE_SOURCE_DIR}/data/dmidiplayer.gresource.xml)
set(GRESOURCE_C ${CMAKE_CURRENT_BINARY_DIR}/dmidiplayer-resources.c)
add_custom_command(
    OUTPUT ${GRESOURCE_C}
    COMMAND ${GLIB_COMPILE_RESOURCES}
            --target=${GRESOURCE_C}
            --sourcedir=${CMAKE_SOURCE_DIR}/data
            --generate-source
            ${GRESOURCE_XML}
    DEPENDS ${GRESOURCE_XML} ${CMAKE_SOURCE_DIR}/data/style.css
    VERBATIM
)

set(CORE_SOURCES
    src/midi/events.cpp
    src/midi/sequence.cpp
    src/midi/output.cpp
    src/midi/player.cpp
    src/midi/sysex.cpp
    src/app/settings.cpp
    src/app/playlist.cpp
)

add_library(dmidi-core STATIC ${CORE_SOURCES})
target_include_directories(dmidi-core PUBLIC ${CMAKE_SOURCE_DIR}/src)
target_link_libraries(dmidi-core PUBLIC
    PkgConfig::GLIB
    PkgConfig::ALSA
    PkgConfig::FLUIDSYNTH
    PkgConfig::UCHARDET
    Threads::Threads
)

add_executable(dmidiplayer
    src/main.cpp
    src/ui/window.cpp
    ${GRESOURCE_C}
)
target_link_libraries(dmidiplayer PRIVATE
    dmidi-core
    PkgConfig::GTK4
    PkgConfig::ADWAITA
)
target_compile_definitions(dmidiplayer PRIVATE
    VERSION="${PROJECT_VERSION}"
    DATADIR="${CMAKE_INSTALL_FULL_DATADIR}"
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
endif()

if(UNIX AND NOT APPLE)
    install(TARGETS dmidiplayer RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
    install(FILES net.sourceforge.dmidiplayer.desktop
            DESTINATION ${CMAKE_INSTALL_DATAROOTDIR}/applications)
    install(FILES net.sourceforge.dmidiplayer.metainfo.xml
            DESTINATION ${CMAKE_INSTALL_DATAROOTDIR}/metainfo)
endif()
