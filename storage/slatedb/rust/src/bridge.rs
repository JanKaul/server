#[cxx::bridge(namespace = "slatedb")]
mod ffi {
    extern "Rust" {
        fn slatedb_version() -> String;
    }
}

fn slatedb_version() -> String {
    format!("slatedb-engine {}", env!("CARGO_PKG_VERSION"))
}
