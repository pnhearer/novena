include(CMakeFindDependencyMacro)

if(NOT TARGET novena::novena)
  add_library(novena::novena UNKNOWN IMPORTED)
  set_target_properties(novena::novena PROPERTIES
    INTERFACE_INCLUDE_DIRECTORIES "${CMAKE_CURRENT_LIST_DIR}/../../include"
    IMPORTED_LOCATION "${CMAKE_CURRENT_LIST_DIR}/../../lib/libnovena.so"
    IMPORTED_IMPLIB "${CMAKE_CURRENT_LIST_DIR}/../../lib/novena.lib")
endif()
