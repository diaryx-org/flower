// swift-tools-version:5.9
//
// The Swift package a SwiftUI app links to drive flower-core. It builds the
// UniFFI binding + the FlowerUI renderer **from source**; the Rust staticlib
// itself is linked by the consuming app via a `-force_load` linker flag and
// (re)built by a pre-build step — see `apps/flower-editor/project.yml`, which does
// exactly that so an Xcode build always picks up fresh Rust changes.
// `bootstrap.sh` generates the two `generated/` inputs below.
//
//   • generated/headers/            the C ABI header + module map (the
//                                   `flower_ffiFFI` clang module the Swift imports)
//   • generated/Sources/FlowerFFI/  the UniFFI-generated Swift over that C ABI
//
// A consumer adds this directory as a local package and links the staticlib:
//   .package(path: "…/packages/flower-swift")        // import FlowerUI
//   OTHER_LDFLAGS = -force_load <path>/libflower_ffi.a
import PackageDescription

let package = Package(
    name: "FlowerFFI",
    platforms: [.macOS(.v13), .iOS(.v16)],
    products: [
        // The low-level binding: `FlowerDoc` + the `DocView`/`RowView` value types.
        .library(name: "FlowerFFI", targets: ["FlowerFFI"]),
        // The SwiftUI tree editor built on it: `FlowerEditor` + `FlowerModel`.
        .library(name: "FlowerUI", targets: ["FlowerUI"]),
    ],
    targets: [
        // The C ABI as a clang module (`import flower_ffiFFI`). No library to link
        // here — the app force-loads the Rust `.a`, so the symbols the generated
        // Swift references stay undefined until the final executable link.
        .systemLibrary(name: "flower_ffiFFI", path: "generated/headers"),
        // The generated Swift, compiled against that C module.
        .target(
            name: "FlowerFFI",
            dependencies: ["flower_ffiFFI"],
            path: "generated/Sources/FlowerFFI"
        ),
        // The reusable SwiftUI editor surface (committed source).
        .target(
            name: "FlowerUI",
            dependencies: ["FlowerFFI"],
            path: "Sources/FlowerUI"
        ),
        // Renderer unit tests. They build `RowView`/`DocView` fixtures in pure
        // Swift, but the module still references the FFI symbols, so the test
        // binary must link the staticlib — `scripts/test-swift.sh` force-loads it.
        .testTarget(
            name: "FlowerUITests",
            dependencies: ["FlowerUI"],
            path: "Tests/FlowerUITests"
        ),
    ]
)
