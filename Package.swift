// swift-tools-version: 6.3
import PackageDescription

let package = Package(
    name: "MinionsCode",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "git@github.com:migueldeicaza/SwiftTerm.git", from: "1.2.0"),
    ],
    targets: [
        .executableTarget(
            name: "MinionsCode",
            dependencies: [
                .product(name: "SwiftTerm", package: "SwiftTerm"),
            ],
            path: "Sources/MinionsCode"
        ),
    ],
    swiftLanguageModes: [.v6]
)
