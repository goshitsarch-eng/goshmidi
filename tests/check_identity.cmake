set(APP_ID "com.goshapps.GoshMIDI")
set(LEGACY_APP_ID "net.sourceforge.dmidiplayer")

foreach(relative_path
        "com.goshapps.GoshMIDI.desktop"
        "com.goshapps.GoshMIDI.metainfo.xml")
    set(path "${PROJECT_SOURCE_DIR}/${relative_path}")
    file(READ "${path}" contents)

    string(FIND "${contents}" "${APP_ID}" canonical_position)
    if(canonical_position EQUAL -1)
        message(FATAL_ERROR "${relative_path} does not contain canonical app ID ${APP_ID}")
    endif()

    string(FIND "${contents}" "${LEGACY_APP_ID}" legacy_position)
    if(NOT legacy_position EQUAL -1)
        message(FATAL_ERROR "${relative_path} still contains legacy app ID ${LEGACY_APP_ID}")
    endif()
endforeach()

file(READ "${PROJECT_SOURCE_DIR}/icons/CMakeLists.txt" icon_install_contents)
string(FIND "${icon_install_contents}" "RENAME \${GOSH_APP_ID}" icon_identity_position)
if(icon_identity_position EQUAL -1)
    message(FATAL_ERROR "Icon installation does not use the canonical GOSH_APP_ID variable")
endif()
string(FIND "${icon_install_contents}" "${LEGACY_APP_ID}" legacy_icon_position)
if(NOT legacy_icon_position EQUAL -1)
    message(FATAL_ERROR "Icon installation still contains legacy app ID ${LEGACY_APP_ID}")
endif()

file(READ "${PROJECT_SOURCE_DIR}/com.goshapps.GoshMIDI.desktop" desktop_contents)
foreach(required_line
        "Icon=${APP_ID}"
        "StartupWMClass=${APP_ID}")
    string(FIND "${desktop_contents}" "${required_line}" required_position)
    if(required_position EQUAL -1)
        message(FATAL_ERROR "Desktop metadata is missing ${required_line}")
    endif()
endforeach()