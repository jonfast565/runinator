cmake_minimum_required(VERSION 3.16)

project(CommandCenter LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)
set(CMAKE_AUTOUIC_SEARCH_PATHS
  ${CMAKE_CURRENT_SOURCE_DIR}/src/ui/windows
  ${CMAKE_CURRENT_SOURCE_DIR}/src/ui/dialogs
)

find_package(Qt6 COMPONENTS Widgets Network QUIET)
if(Qt6_FOUND)
  set(QT_PACKAGE Qt6)
else()
  find_package(Qt5 COMPONENTS Widgets Network REQUIRED)
  set(QT_PACKAGE Qt5)
endif()

add_executable(CommandCenter
  src/main.cpp
  src/windows/main_window.cpp
  src/dialogs/task_editor_dialog.cpp
  src/clients/api_client.cpp
  src/gossip/gossip_discovery.cpp
  src/models/scheduled_task.cpp
  src/ui/windows/main_window.ui
  src/ui/dialogs/task_editor_dialog.ui
)

target_link_libraries(CommandCenter PRIVATE ${QT_PACKAGE}::Widgets ${QT_PACKAGE}::Network)

target_include_directories(CommandCenter PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src)

set_target_properties(CommandCenter PROPERTIES
  WIN32_EXECUTABLE ON
  MACOSX_BUNDLE ON
)
