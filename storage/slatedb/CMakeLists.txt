find_package(Corrosion REQUIRED)

corrosion_import_crate(
  MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/rust/Cargo.toml
  CRATE_TYPES staticlib
)

corrosion_add_cxxbridge(slatedb_bridge
  CRATE slatedb_engine
  FILES bridge.rs
)

# The cxx-generated `bridge.cc` `#include`s `slatedb_field_callbacks.h`.
# Add shim/ to its include path, plus MariaDB's sql/ and include/ so
# the transitive `field.h` / `table.h` includes resolve.
target_include_directories(slatedb_bridge PRIVATE
  ${CMAKE_CURRENT_SOURCE_DIR}/shim
  ${CMAKE_SOURCE_DIR}/sql
  ${CMAKE_SOURCE_DIR}/include
)

SET(SLATEDB_SOURCES shim/ha_slatedb.cc)
MYSQL_ADD_PLUGIN(slatedb ${SLATEDB_SOURCES} STORAGE_ENGINE MODULE_ONLY
  COMPONENT Storage
  LINK_LIBRARIES slatedb_bridge
)
# bridge.h (cxx-generated) includes slatedb_field_callbacks.h; add
# shim/ so the plugin target can resolve that transitive include.
target_include_directories(slatedb PRIVATE
  ${CMAKE_CURRENT_SOURCE_DIR}/shim
  ${CMAKE_SOURCE_DIR}/sql
  ${CMAKE_SOURCE_DIR}/include
)
