// swift-tools-version:5.9
import PackageDescription

// Rs Editor for iOS and macOS: a UIView/NSView drawn by the Rust core with
// Skia on Metal. Run ../package.sh first; it builds RsEditorCore (the Rust
// core as an XCFramework with its Swift bindings) into ../generated.
let package = Package(
    name: "RsEditorKit",
    platforms: [
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .library(name: "RsEditorKit", targets: ["RsEditorKit"]),
    ],
    dependencies: [
        .package(path: "../generated/RsEditorCore"),
    ],
    targets: [
        .target(
            name: "RsEditorKit",
            dependencies: [.product(name: "RsEditorCore", package: "RsEditorCore")],
            linkerSettings: [
                // Skia's Metal backend and its font manager (Core Text).
                .linkedFramework("Metal"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("CoreText"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("ImageIO"),
                .linkedLibrary("c++"),
            ]
        ),
        .testTarget(name: "RsEditorKitTests", dependencies: ["RsEditorKit"]),
    ]
)
