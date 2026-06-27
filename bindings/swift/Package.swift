// swift-tools-version:5.9
//
// RustPdf — idiomatic Swift binding for the rust-pdf core over its C ABI
// (libpdf_ffi). Pure FFI: the cdylib is located and bound at run time via
// `dlopen`/`dlsym` (see Native.swift), so there is no link-time dependency and
// the same package works from the repo checkout and from an installed library
// path. Build the native library first with `cargo build -p pdf-ffi`.
import PackageDescription

let package = Package(
    name: "RustPdf",
    platforms: [
        .macOS(.v11)
    ],
    products: [
        .library(name: "RustPdf", targets: ["RustPdf"]),
        .executable(name: "rustpdf-example", targets: ["Example"])
    ],
    targets: [
        .target(
            name: "RustPdf",
            path: "Sources/RustPdf"
        ),
        .executableTarget(
            name: "Example",
            dependencies: ["RustPdf"],
            path: "Sources/Example"
        ),
        .testTarget(
            name: "RustPdfTests",
            dependencies: ["RustPdf"],
            path: "Tests/RustPdfTests"
        )
    ]
)
