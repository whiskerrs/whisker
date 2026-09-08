// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "CngTestWidget",
    products: [.library(name: "CngTestWidget", targets: ["CngTestWidget"])],
    targets: [.target(name: "CngTestWidget", path: "ios")]
)
