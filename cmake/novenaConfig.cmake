# Installed as lib/cmake/novena/novenaConfig.cmake inside a release archive.
get_filename_component(_novena_prefix "${CMAKE_CURRENT_LIST_DIR}/../../.." ABSOLUTE)

if(NOT TARGET novena::novena)
  add_library(novena::novena SHARED IMPORTED)
  set_target_properties(novena::novena PROPERTIES
    INTERFACE_INCLUDE_DIRECTORIES "${_novena_prefix}/include")
  if(WIN32)
    set_target_properties(novena::novena PROPERTIES
      IMPORTED_LOCATION "${_novena_prefix}/lib/novena.dll"
      IMPORTED_IMPLIB "${_novena_prefix}/lib/novena.lib")
  else()
    set_target_properties(novena::novena PROPERTIES
      IMPORTED_LOCATION "${_novena_prefix}/lib/libnovena.so")
  endif()
endif()

unset(_novena_prefix)
