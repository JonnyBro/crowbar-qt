cmake_minimum_required(VERSION 3.21)
project(crowbar-qt VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

find_package(Qt6 6.6 REQUIRED COMPONENTS Core Widgets)
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
    src/core/Steam.cpp
    src/core/Steam.h
    src/core/Studiomdl.cpp
    src/core/Studiomdl.h
    src/core/ToolProcess.cpp
    src/core/ToolProcess.h
)
target_include_directories(crowbar_core PUBLIC src)
target_link_libraries(crowbar_core PUBLIC Qt6::Core PRIVATE LibLZMA::LibLZMA)

qt_add_executable(crowbar-qt
    src/main.cpp
    src/ui/MainWindow.cpp
    src/ui/MainWindow.h
    src/ui/CompileTab.cpp
    src/ui/CompileTab.h
    src/ui/GameCombo.cpp
    src/ui/GameCombo.h
    src/ui/GameList.cpp
    src/ui/GameList.h
    src/ui/GamesTab.cpp
    src/ui/GamesTab.h
    src/ui/PackTab.cpp
    src/ui/PackTab.h
    src/ui/UnpackTab.cpp
    src/ui/UnpackTab.h
    src/ui/ViewTab.cpp
    src/ui/ViewTab.h
)
target_link_libraries(crowbar-qt PRIVATE crowbar_core Qt6::Widgets)
target_compile_definitions(crowbar-qt PRIVATE CROWBAR_VERSION="${PROJECT_VERSION}")

install(TARGETS crowbar-qt)

include(CTest)
if(BUILD_TESTING)
    find_package(Qt6 REQUIRED COMPONENTS Test)
    foreach(test tst_vdf tst_steam tst_launcher tst_package)
        qt_add_executable(${test} tests/${test}.cpp)
        target_link_libraries(${test} PRIVATE crowbar_core Qt6::Test)
        add_test(NAME ${test} COMMAND ${test})
    endforeach()
endif()
