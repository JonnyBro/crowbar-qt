cmake_minimum_required(VERSION 3.21)
project(crowbar-qt VERSION 1.2.1 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

find_package(Qt6 6.6 REQUIRED COMPONENTS Core Gui Network Svg Widgets)
find_package(LibLZMA REQUIRED)
qt_standard_project_setup()

add_compile_options(-Wall -Wextra)

add_library(crowbar_core STATIC
    src/core/Vdf.cpp
    src/core/Vdf.h
    src/core/Hlmv.cpp
    src/core/Hlmv.h
    src/core/Launcher.cpp
    src/core/Launcher.h
    src/core/Package.cpp
    src/core/Package.h
    src/core/Packer.cpp
    src/core/Packer.h
    src/core/Publisher.cpp
    src/core/Publisher.h
    src/core/Runner.cpp
    src/core/Runner.h
    src/core/Steam.cpp
    src/core/Steam.h
    src/core/Studiomdl.cpp
    src/core/Studiomdl.h
    src/core/ToolProcess.cpp
    src/core/Workshop.cpp
    src/core/Workshop.h
    src/core/ToolProcess.h
)
target_include_directories(crowbar_core PUBLIC src)
target_link_libraries(crowbar_core PUBLIC Qt6::Core Qt6::Gui Qt6::Network PRIVATE LibLZMA::LibLZMA)

qt_add_executable(crowbar-qt
    src/main.cpp
    src/ui/About.cpp
    src/ui/About.h
    src/ui/CompilePage.cpp
    src/ui/CompilePage.h
    src/ui/DownloadPage.cpp
    src/ui/DownloadPage.h
    src/ui/GameList.cpp
    src/ui/GameList.h
    src/ui/GamesPage.cpp
    src/ui/GamesPage.h
    src/ui/LogView.cpp
    src/ui/LogView.h
    src/ui/MainWindow.cpp
    src/ui/MainWindow.h
    src/ui/PackPage.cpp
    src/ui/PackPage.h
    src/ui/PackagesPage.cpp
    src/ui/PackagesPage.h
    src/ui/Page.cpp
    src/ui/Page.h
    src/ui/PathField.cpp
    src/ui/PathField.h
    src/ui/SettingsPage.cpp
    src/ui/SettingsPage.h
    src/ui/Tasks.cpp
    src/ui/Tasks.h
    src/ui/TasksPanel.cpp
    src/ui/TasksPanel.h
    src/ui/TempDir.cpp
    src/ui/TempDir.h
    src/ui/ViewPage.cpp
    src/ui/ViewPage.h
    src/ui/WorkshopPage.cpp
    src/ui/WorkshopPage.h
)
target_link_libraries(crowbar-qt PRIVATE crowbar_core Qt6::Widgets Qt6::Svg)
# Builds from a git checkout that is not exactly the release tag name their commit,
# so a crowbar-qt-git install can be told apart from the release with the same number.
set(CROWBAR_COMMIT "")
find_package(Git QUIET)
if(GIT_FOUND AND EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/.git")
    execute_process(COMMAND "${GIT_EXECUTABLE}" describe --tags --exact-match --match "v${PROJECT_VERSION}"
                    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}" RESULT_VARIABLE CROWBAR_ON_TAG
                    OUTPUT_QUIET ERROR_QUIET)
    if(NOT CROWBAR_ON_TAG EQUAL 0)
        execute_process(COMMAND "${GIT_EXECUTABLE}" rev-parse --short=7 HEAD
                        WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}" OUTPUT_VARIABLE CROWBAR_COMMIT
                        OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
    endif()
endif()
target_compile_definitions(crowbar-qt PRIVATE CROWBAR_VERSION="${PROJECT_VERSION}" CROWBAR_COMMIT="${CROWBAR_COMMIT}")

find_package(Qt6 REQUIRED COMPONENTS LinguistTools)
qt_add_translations(crowbar-qt
    TS_FILES translations/crowbar-qt_ru.ts
    SOURCE_TARGETS crowbar-qt crowbar_core
    RESOURCE_PREFIX /i18n
    LUPDATE_OPTIONS -no-obsolete -locations none
)

include(GNUInstallDirs)
install(TARGETS crowbar-qt)
install(FILES packaging/crowbar-qt.desktop DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
install(FILES packaging/crowbar-qt-mime.xml DESTINATION ${CMAKE_INSTALL_DATADIR}/mime/packages RENAME crowbar-qt.xml)

# The icon is optional: packaging/crowbar-qt.svg or packaging/crowbar-qt.png, whichever exists.
# Without one the app and its menu entry use the desktop's generic icon.
if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/packaging/crowbar-qt.svg)
    set(CROWBAR_ICON crowbar-qt.svg)
    install(FILES packaging/crowbar-qt.svg DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps)
elseif(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/packaging/crowbar-qt.png)
    set(CROWBAR_ICON crowbar-qt.png)
    install(FILES packaging/crowbar-qt.png DESTINATION ${CMAKE_INSTALL_DATADIR}/pixmaps)
endif()
if(CROWBAR_ICON)
    qt_add_resources(crowbar-qt "icon" PREFIX / BASE packaging FILES packaging/${CROWBAR_ICON})
    target_compile_definitions(crowbar-qt PRIVATE CROWBAR_ICON=":/${CROWBAR_ICON}")
endif()

include(CTest)
if(BUILD_TESTING)
    find_package(Qt6 REQUIRED COMPONENTS Test)
    foreach(test tst_vdf tst_steam tst_launcher tst_package tst_publisher tst_workshop)
        qt_add_executable(${test} tests/${test}.cpp)
        target_link_libraries(${test} PRIVATE crowbar_core Qt6::Test)
        add_test(NAME ${test} COMMAND ${test})
    endforeach()
endif()
