import Foundation

/// Intrinsic-measurement policy negotiated with the Rust runtime.
public enum WhiskerMeasurement: Hashable {
    case none, text, replacedContent, custom
}

/// Rust-owned child semantics. Native mount targets remain Host-local.
public enum WhiskerChildPolicy: Hashable {
    case none, elements, plainText

    public var acceptsElements: Bool { self == .elements }
    public var acceptsPlainText: Bool { self == .plainText }
}

/// Top-level shape of a value carried by `WhiskerValue`.
public enum WhiskerValueKind: Hashable {
    case null, bool, int, float, string, bytes, array, map
}

/// Runtime-assigned property identity received during surface bootstrap.
public struct WhiskerPropertyBinding: Hashable {
    public let id: Int
    public let name: String
    public let value: WhiskerValueKind

    public init(id: Int, name: String, value: WhiskerValueKind) {
        precondition(id > 0 && !name.isEmpty)
        self.id = id
        self.name = name
        self.value = value
    }
}

/// Runtime-assigned event identity received during surface bootstrap.
public struct WhiskerEventBinding: Hashable {
    public let id: Int
    public let name: String
    public let detail: WhiskerValueKind?

    public init(id: Int, name: String, detail: WhiskerValueKind? = nil) {
        precondition(id > 0 && !name.isEmpty)
        self.id = id
        self.name = name
        self.detail = detail
    }
}

/// Runtime-assigned command identity received during surface bootstrap.
public struct WhiskerCommandBinding: Hashable {
    public let id: Int
    public let name: String
    public let arguments: WhiskerValueKind

    public init(id: Int, name: String, arguments: WhiskerValueKind) {
        precondition(id > 0 && !name.isEmpty)
        self.id = id
        self.name = name
        self.arguments = arguments
    }
}

/// Rust-owned runtime registration, not generated Swift source.
public struct WhiskerElementRegistration {
    public let elementType: Int
    public let name: String
    public let childPolicy: WhiskerChildPolicy
    public let measurement: WhiskerMeasurement
    public let textStyle: Bool
    public let properties: [WhiskerPropertyBinding]
    public let events: [WhiskerEventBinding]
    public let commands: [WhiskerCommandBinding]

    public init(
        elementType: Int,
        name: String,
        childPolicy: WhiskerChildPolicy,
        measurement: WhiskerMeasurement,
        textStyle: Bool = false,
        properties: [WhiskerPropertyBinding] = [],
        events: [WhiskerEventBinding] = [],
        commands: [WhiskerCommandBinding] = []
    ) {
        precondition(elementType > 0)
        precondition(!name.isEmpty && !name.contains("@"))
        precondition(Set(properties.map(\.id)).count == properties.count)
        precondition(Set(properties.map(\.name)).count == properties.count)
        precondition(Set(events.map(\.id)).count == events.count)
        precondition(Set(events.map(\.name)).count == events.count)
        precondition(Set(commands.map(\.id)).count == commands.count)
        precondition(Set(commands.map(\.name)).count == commands.count)
        self.elementType = elementType
        self.name = name
        self.childPolicy = childPolicy
        self.measurement = measurement
        self.textStyle = textStyle
        self.properties = properties
        self.events = events
        self.commands = commands
    }
}
