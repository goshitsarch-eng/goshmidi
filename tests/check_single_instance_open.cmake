file(READ "${PROJECT_SOURCE_DIR}/src/main.cpp" main_source)

foreach(required_fragment
        "G_APPLICATION_HANDLES_OPEN"
        "g_signal_connect(app, \"open\""
        "g_file_get_path(files[i])"
        "applicationArgs.insert(applicationArgs.end(), files.begin(), files.end())"
        "applicationArgv.data()")
    string(FIND "${main_source}" "${required_fragment}" fragment_position)
    if(fragment_position EQUAL -1)
        message(FATAL_ERROR "Single-instance file-open contract is missing: ${required_fragment}")
    endif()
endforeach()

string(FIND "${main_source}" "g_application_run(G_APPLICATION(app), 0, nullptr)" legacy_run_position)
if(NOT legacy_run_position EQUAL -1)
    message(FATAL_ERROR "GApplication still discards command-line file arguments")
endif()