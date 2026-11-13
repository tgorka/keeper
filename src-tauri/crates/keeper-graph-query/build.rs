use std::path::{Path, PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let os = std::env::var("CARGO_CFG_TARGET_OS")?;
    if os != "linux" && os != "macos" {
        return Ok(());
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH")?;
    if arch != "x86_64" && arch != "aarch64" {
        return Err("unsupported graph worker architecture".into());
    }
    let vendor = Path::new("vendor/graphqlite");
    let mut c = cc::Build::new();
    c.include(std::env::var("DEP_SQLITE3_INCLUDE")?)
        .include(vendor.join("src/include"))
        .include(vendor.join("generated"))
        .define("SQLITE_CORE", None)
        .define("_GNU_SOURCE", None)
        .define("YYSTACK_USE_ALLOCA", "0")
        .define("YYMAXDEPTH", "4096")
        .flag("-std=c11")
        .flag("-include")
        .flag("native-allocation.h")
        .warnings(false);
    let mut sources = vec![vendor.join("src/extension.c")];
    for directory in ["parser", "transform", "executor", "runtime"] {
        for entry in std::fs::read_dir(vendor.join("src/backend").join(directory))? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "c") {
                sources.push(path);
            }
        }
    }
    sources.extend([
        PathBuf::from("vendor/graphqlite/generated/cypher_gram.tab.c"),
        PathBuf::from("vendor/graphqlite/generated/cypher_scanner.c"),
    ]);
    sources.sort();
    for path in sources {
        c.file(path);
    }
    c.compile("keeper_graphqlite");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rerun-if-changed=vendor/graphqlite");
    println!("cargo:rerun-if-changed=native-allocation.h");
    Ok(())
}
