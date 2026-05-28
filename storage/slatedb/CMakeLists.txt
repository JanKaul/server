find_package(Corrosion REQUIRED)

corrosion_import_crate(
  MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/rust/Cargo.toml
  CRATE_TYPES staticlib
)

corrosion_add_cxxbridge(slatedb_bridge
  CRATE slatedb_engine
  FILES bridge.rs
)

SET(SLATEDB_SOURCES shim/ha_slatedb.cc)
MYSQL_ADD_PLUGIN(slatedb ${SLATEDB_SOURCES} STORAGE_ENGINE MODULE_ONLY
  COMPONENT Storage
  LINK_LIBRARIES slatedb_bridge
)
