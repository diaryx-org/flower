// swift-tools-version:5.9
//
// The Swift package a SwiftUI app links to drive flower-core. The manifest
// lives at the repository root — not in packages/flower-swift/ — because
// SwiftPM resolves git dependencies only from a root Package.swift, and
// by-version resolution is how a consumer is meant to take this package:
//
//   .package(url: "https://github.com/diaryx-org/flower.git", from: "X.Y.Z")
//
// (A local checkout still works: `.package(path: "…/flower")` — the repo root,
// not packages/flower-swift/.) The sources stay under packages/flower-swift/;
// only the manifest sits here.
//
// It builds the UniFFI binding + the FlowerUI renderer **from source**; the
// Rust staticlib itself is linked by the consuming app via a `-force_load`
// linker flag and (re)built by a pre-build step — see
// `apps/flower-editor/project.yml`, which does exactly that so an Xcode build
// always picks up fresh Rust changes:
//   OTHER_LDFLAGS = -force_load <path>/libflower_ffi.a
//
// The two `uniffi-generated/` inputs below are committed — a version-resolved
// clone runs no generators, so they must build as-is. `scripts/gen-bindings.sh`
// writes them from crates/flower-ffi and CI holds them to it (the `bindings`
// job runs `--check`):
//
//   • packages/flower-swift/uniffi-generated/headers/            the C ABI
//     header + module map (the `flower_ffiFFI` clang module the Swift imports)
//   • packages/flower-swift/uniffi-generated/Sources/FlowerFFI/  the UniFFI-
//     generated Swift over that C ABI
import PackageDescription

let package = Package(
    name: "FlowerFFI",
    platforms: [.macOS(.v13), .iOS(.v16)],
    products: [
        // The low-level binding: `FlowerDoc` + the `PagesView`/`DocView` value types.
        .library(name: "FlowerFFI", targets: ["FlowerFFI"]),
        // The model over that binding: `FlowerModel`, plus the conformances that
        // let the page editor render its records. Re-exports FlowerPagesUI, so
        // one `import FlowerUI` gets the whole editor.
        .library(name: "FlowerUI", targets: ["FlowerUI"]),
        // The page editor, with no binding behind it: `FlowerPages` and the
        // protocols it renders. A host with its own UniFFI records conforms them
        // and links this alone — no second staticlib, no second namespace.
        .library(name: "FlowerPagesUI", targets: ["FlowerPagesUI"]),
    ],
    targets: [
        // The C ABI as a clang module (`import flower_ffiFFI`). No library to link
        // here — the app force-loads the Rust `.a`, so the symbols the generated
        // Swift references stay undefined until the final executable link.
        .systemLibrary(
            name: "flower_ffiFFI",
            path: "packages/flower-swift/uniffi-generated/headers"
        ),
        // The generated Swift, compiled against that C module.
        .target(
            name: "FlowerFFI",
            dependencies: ["flower_ffiFFI"],
            path: "packages/flower-swift/uniffi-generated/Sources/FlowerFFI"
        ),
        // The page editor and the presentation vocabulary both surfaces share.
        // **No FFI dependency**, by design: it is written against the protocols
        // in PageProtocols.swift, which this package's generated records satisfy
        // as they are and another host's records satisfy with an extension.
        .target(
            name: "FlowerPagesUI",
            path: "packages/flower-swift/Sources/FlowerPagesUI"
        ),
        // The committed-source half over the binding: the model, and the
        // conformances that let the page editor render this package's records.
        .target(
            name: "FlowerUI",
            dependencies: ["FlowerFFI", "FlowerPagesUI"],
            path: "packages/flower-swift/Sources/FlowerUI"
        ),
        // Renderer unit tests. They build `RowView`/`DocView` fixtures in pure
        // Swift, but the module still references the FFI symbols, so the test
        // binary must link the staticlib — `scripts/test-swift.sh` force-loads it.
        // `FlowerPagesUI` is named directly, not leaned on transitively:
        // ForeignHostTests.swift renders the page editor over records of its own
        // with no binding in sight, which is the test that the split is real.
        .testTarget(
            name: "FlowerUITests",
            dependencies: ["FlowerUI", "FlowerPagesUI"],
            path: "packages/flower-swift/Tests/FlowerUITests"
        ),
    ]
)
