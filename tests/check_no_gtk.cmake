# Nothing GTK or libadwaita may remain: the interface is Qt 6 and Kirigami.
file(GLOB_RECURSE tracked_sources
     "${PROJECT_SOURCE_DIR}/src/*.cpp"
     "${PROJECT_SOURCE_DIR}/src/*.hpp"
     "${PROJECT_SOURCE_DIR}/src/*.qml"
     "${PROJECT_SOURCE_DIR}/CMakeLists.txt")

foreach(source IN LISTS tracked_sources)
    file(READ "${source}" contents)
    foreach(forbidden "gtk/gtk.h" "adwaita.h" "GtkWidget" "AdwApplication" "libadwaita"
                      "gtk_widget_" "adw_" "g_signal_connect" "GKeyFile" "g_idle_add")
        string(FIND "${contents}" "${forbidden}" position)
        if(NOT position EQUAL -1)
            message(FATAL_ERROR "${source} still references ${forbidden}")
        endif()
    endforeach()
endforeach()

file(READ "${PROJECT_SOURCE_DIR}/CMakeLists.txt" build_contents)
foreach(required "Qt6" "KF6" "Kirigami" "qt_add_qml_module")
    string(FIND "${build_contents}" "${required}" position)
    if(position EQUAL -1)
        message(FATAL_ERROR "The build does not use ${required}")
    endif()
endforeach()

if(EXISTS "${PROJECT_SOURCE_DIR}/data/style.css")
    message(FATAL_ERROR "The GTK stylesheet data/style.css is still present")
endif()
if(EXISTS "${PROJECT_SOURCE_DIR}/src/ui/window.cpp")
    message(FATAL_ERROR "The GTK main window is still present")
endif()
