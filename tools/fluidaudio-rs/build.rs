//! Builds the Swift bridge with SwiftPM and links it statically.
//!
//! Only a macOS target builds anything: on every other target the crate
//! compiles to its pure-Rust types alone (the FFI module is `cfg`-gated), so
//! its unit tests run anywhere.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=swift/");
    println!("cargo:rerun-if-changed=Package.swift");
    println!("cargo:rerun-if-changed=Package.resolved");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let out_dir = PathBuf::from(env("OUT_DIR"));
    let manifest_dir = PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let arch = match env("CARGO_CFG_TARGET_ARCH").as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("fluidaudio-rs: unsupported macOS architecture {other}"),
    };
    let build_dir = out_dir.join("swift-build");

    let status = Command::new("swift")
        .args(["build", "-c", "release", "--arch", arch, "--build-path"])
        .arg(&build_dir)
        .current_dir(&manifest_dir)
        .status()
        .unwrap_or_else(|error| panic!("fluidaudio-rs: cannot run `swift build`: {error}"));
    if !status.success() {
        panic!("fluidaudio-rs: `swift build` failed");
    }

    // `<build>/release` links to the products under both SwiftPM build
    // systems (`<triple>/release` natively, `out/Products/Release` with
    // swift-build, the default in Swift 6.4).
    let products = build_dir.join("release");
    println!("cargo:rustc-link-search=native={}", products.display());
    println!("cargo:rustc-link-lib=static=FluidAudioBridge");

    for framework in [
        "Foundation",
        "AVFoundation",
        "CoreMedia",
        "CoreAudio",
        "CoreML",
        "Accelerate",
        "Metal",
        "MetalPerformanceShaders",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
    // FluidAudio's FastClusterWrapper is C++.
    println!("cargo:rustc-link-lib=c++");

    // The Swift objects auto-link the Swift runtime (`-lswiftCore`,
    // `-lswift_Concurrency`, …) against the SDK's .tbd stubs and the
    // toolchain's static compatibility libraries. Linked for a deployment
    // target below macOS 12 (rustc's aarch64 default is 11.0, keeper's
    // bundle floor too), `libswift_Concurrency` is `@rpath/…`, and a binary
    // without an rpath to the OS copy in /usr/lib/swift aborts in dyld.
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
    println!(
        "cargo:rustc-link-search=native={}",
        Path::new(&sdk).join("usr/lib/swift").display()
    );
    let swiftc = xcrun(&["--find", "swiftc"]);
    if let Some(toolchain_usr) = Path::new(&swiftc).parent().and_then(Path::parent) {
        println!(
            "cargo:rustc-link-search=native={}",
            toolchain_usr.join("lib/swift/macosx").display()
        );
    }
    // Reaches this crate's own tests only: Cargo never forwards a
    // dependency's link args, so every binary that links this crate must
    // emit the same `-Wl,-rpath,/usr/lib/swift` from its own build script.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("fluidaudio-rs: {key} is not set"))
}

fn xcrun(args: &[&str]) -> String {
    let output = Command::new("xcrun")
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("fluidaudio-rs: cannot run xcrun: {error}"));
    if !output.status.success() {
        panic!("fluidaudio-rs: `xcrun {}` failed", args.join(" "));
    }
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
