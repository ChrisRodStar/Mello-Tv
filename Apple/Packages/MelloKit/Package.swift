// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MelloKit",
    platforms: [
        .macOS(.v13),
        .iOS(.v17)
    ],
    products: [
        .library(
            name: "MelloKit",
            targets: ["MelloKit"]
        ),
    ],
    targets: [
        .target(
            name: "MelloKit",
            dependencies: [],
            resources: [
                .process("Resources")
            ]
        ),
        .testTarget(
            name: "MelloKitTests",
            dependencies: ["MelloKit"]
        ),
    ]
)

