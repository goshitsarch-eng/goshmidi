# The single-instance file-open contract: one running window, and any further
# launch (or D-Bus activation) hands its files to it instead of opening a
# second window.
file(READ "${PROJECT_SOURCE_DIR}/src/main.cpp" main_source)

foreach(required_fragment
        "KDBusService service(KDBusService::Unique)"
        "KDBusService::activateRequested"
        "locatorsFrom(parser.positionalArguments()"
        "controller->openLocators(startupFiles, true)"
        "controller->openLocators(locators, true)"
        "requestActivate()")
    string(FIND "${main_source}" "${required_fragment}" fragment_position)
    if(fragment_position EQUAL -1)
        message(FATAL_ERROR "Single-instance file-open contract is missing: ${required_fragment}")
    endif()
endforeach()

# Remote locators must reach the resolver untouched rather than being dropped
# for having no local path — that was the network-share bug.
foreach(required_fragment
        "RemoteFileResolver::locatorFromUrl(url)"
        "QUrl::AssumeLocalFile")
    string(FIND "${main_source}" "${required_fragment}" fragment_position)
    if(fragment_position EQUAL -1)
        message(FATAL_ERROR "Remote file-open contract is missing: ${required_fragment}")
    endif()
endforeach()
