find_package(Corrosion REQUIRED)

# `field_callbacks` enables the cxx::bridge Field/TABLE callback
# surface in `src/bridge_field.rs`. It's off by default in Cargo so
# `cargo test --lib` works without a C++ build context; here we turn
# it on for the real plugin build so the bridge generates the C++
# trampolines that `shim/slatedb_field_callbacks.h` implements.
corrosion_import_crate(
  MANIFEST_PATH ${CMAKE_CURRENT_SOURCE_DIR}/rust/Cargo.toml
  CRATE_TYPES staticlib
  FEATURES field_callbacks
)

corrosion_add_cxxbridge(slatedb_bridge
  CRATE slatedb_engine
  FILES bridge.rs bridge_field.rs
)

# The cxx-generated `bridge_field.cc` `#include`s
# `slatedb_field_callbacks.h` (header-only inline forwards to
# MariaDB Field methods). Add the shim/ directory to its include
# path, plus MariaDB's sql/ and include/ so the transitive
# `field.h` / `table.h` includes resolve.
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
