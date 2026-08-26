import Foundation
import UIKit

public typealias WhiskerElementEventSink = (WhiskerEventBinding, WhiskerValue) -> Void

public struct WhiskerMeasureRequest {
    public let availableWidth: CGFloat?
    public let availableHeight: CGFloat?
    public let knownWidth: CGFloat?
    public let knownHeight: CGFloat?
    public let payloadVersion: UInt16
    public let payload: Data

    public init(
        availableWidth: CGFloat?,
        availableHeight: CGFloat?,
        knownWidth: CGFloat?,
        knownHeight: CGFloat?,
        payloadVersion: UInt16,
        payload: Data
    ) {
        self.availableWidth = availableWidth
        self.availableHeight = availableHeight
        self.knownWidth = knownWidth
        self.knownHeight = knownHeight
        self.payloadVersion = payloadVersion
        self.payload = payload
    }
}

public struct WhiskerMeasuredSize {
    public let width: CGFloat
    public let height: CGFloat

    public init(width: CGFloat, height: CGFloat) {
        self.width = width
        self.height = height
    }
}

public struct WhiskerTextContent {
    public let value: String
    public let fontSize: CGFloat
    public let fontWeight: Int
    public let color: UIColor
    public let alignment: WhiskerTextAlignment
    public let decoration: WhiskerTextDecoration?
    public let shadow: WhiskerTextShadow?

    public init(
        value: String,
        fontSize: CGFloat,
        fontWeight: Int,
        color: UIColor,
        alignment: WhiskerTextAlignment = .start,
        decoration: WhiskerTextDecoration? = nil,
        shadow: WhiskerTextShadow? = nil
    ) {
        self.value = value
        self.fontSize = fontSize
        self.fontWeight = fontWeight
        self.color = color
        self.alignment = alignment
        self.decoration = decoration
        self.shadow = shadow
    }
}

public enum WhiskerTextAlignment: Equatable { case start, end, left, right, center }

public struct WhiskerTextDecoration {
    public let line: WhiskerTextDecorationLine
    public let style: WhiskerTextDecorationStyle
    public let color: UIColor

    public init(
        line: WhiskerTextDecorationLine,
        style: WhiskerTextDecorationStyle,
        color: UIColor
    ) {
        self.line = line
        self.style = style
        self.color = color
    }
}

public enum WhiskerTextDecorationLine: Equatable { case underline, lineThrough }
public enum WhiskerTextDecorationStyle: Equatable { case solid, double, dotted, dashed, wavy }

public struct WhiskerTextShadow {
    public let offset: CGSize
    public let blurRadius: CGFloat
    public let color: UIColor

    public init(offset: CGSize, blurRadius: CGFloat, color: UIColor) {
        self.offset = offset
        self.blurRadius = blurRadius
        self.color = color
    }
}

public final class WhiskerMountedElement {
    public let registration: WhiskerElementRegistration
    public let view: UIView
    private let nativeElement: WhiskerNativeElement?
    private let textUpdater: ((UIView, WhiskerTextContent) -> Void)?
    private let childrenHostProvider: ((UIView) -> UIView)?
    private let properties: [Int: WhiskerPropComponent]
    private let eventsByName: [String: WhiskerEventBinding]
    private let eventSink: WhiskerElementEventSink
    private var eventMask: UInt64 = 0

    fileprivate init(
        registration: WhiskerElementRegistration,
        view: UIView,
        textUpdater: ((UIView, WhiskerTextContent) -> Void)?,
        childrenHost: ((UIView) -> UIView)?,
        properties: [Int: WhiskerPropComponent],
        eventsByName: [String: WhiskerEventBinding],
        eventSink: @escaping WhiskerElementEventSink
    ) {
        self.registration = registration
        self.view = view
        self.nativeElement = view as? WhiskerNativeElement
        self.textUpdater = textUpdater
        self.childrenHostProvider = childrenHost
        self.properties = properties
        self.eventsByName = eventsByName
        self.eventSink = eventSink
        nativeElement?.installWhiskerEventSink { [weak self] name, detail in
            guard let self, let event = self.eventsByName[name] else { return }
            let bit = UInt64(1) << UInt64(event.id - 1)
            if self.eventMask & bit != 0 { self.eventSink(event, detail) }
        }
    }

    public func setProperty(_ id: Int, value: WhiskerValue) { properties[id]?.setter(view, value) }

    /// Clear is a distinct protocol operation; it is not converted to `.null`.
    public func clearProperty(_ id: Int) { properties[id]?.clearer(view) }

    public func setEventMask(_ mask: UInt64) { eventMask = mask }

    @discardableResult
    public func setText(_ content: WhiskerTextContent) -> Bool {
        guard let textUpdater else { return false }
        textUpdater(view, content)
        return true
    }

    public func childrenHost() -> UIView? { childrenHostProvider?(view) }
    public func dispose() { nativeElement?.installWhiskerEventSink(nil) }
}

/** Host-owned declaration. It contains names and behavior, never Rust IDs. */
public struct WhiskerElementFactory {
    public let name: String
    fileprivate let textUpdater: ((UIView, WhiskerTextContent) -> Void)?
    fileprivate let childrenHost: ((UIView) -> UIView)?
    fileprivate let measurer: ((WhiskerMeasureRequest) -> WhiskerMeasuredSize?)?
    fileprivate let makeView: () -> UIView

    public init(
        name: String,
        textUpdater: ((UIView, WhiskerTextContent) -> Void)? = nil,
        childrenHost: ((UIView) -> UIView)? = nil,
        measurer: ((WhiskerMeasureRequest) -> WhiskerMeasuredSize?)? = nil,
        makeView: @escaping () -> UIView
    ) {
        precondition(!name.isEmpty && !name.contains("@"))
        self.name = name
        self.textUpdater = textUpdater
        self.childrenHost = childrenHost
        self.measurer = measurer
        self.makeView = makeView
    }
}

private struct WhiskerDeclaredElement {
    let factory: WhiskerElementFactory
    let properties: [String: WhiskerPropComponent]
    let events: Set<String>
}

private struct WhiskerBoundElement {
    let registration: WhiskerElementRegistration
    let factory: WhiskerElementFactory
    let properties: [Int: WhiskerPropComponent]
}

public enum WhiskerModuleRegistry {
    private static var modules: [String: Module] = [:]

    fileprivate static func register(_ module: Module) {
        guard let name = module.qualifiedName else { preconditionFailure("module needs a name") }
        modules[name] = module
        WhiskerModuleEventCenter.register(module)
    }

    public static func module(named name: String) -> Module? { modules[name] }
}

/** Process-wide Host declaration registry and per-surface negotiated table. */
public enum WhiskerElementRegistry {
    private static var declarations: [String: WhiskerDeclaredElement] = [:]
    private static var boundByType: [Int: WhiskerBoundElement] = [:]

    public static func register(_ factory: WhiskerElementFactory) {
        register(factory, properties: [:], events: [])
    }

    fileprivate static func register(_ view: WhiskerViewComponent, fallbackName: String) {
        let name = view.elementName ?? fallbackName
        if let factory = view.factory {
            let props = view.components.compactMap { $0 as? WhiskerPropComponent }
            let properties = Dictionary(uniqueKeysWithValues: props.map { ($0.name, $0) })
            let events = Set(view.components.compactMap { $0 as? WhiskerEventsComponent }.flatMap(\.names))
            register(factory, properties: properties, events: events)
            return
        }
        guard let viewClass = view.viewClass else {
            preconditionFailure("\(name) View declaration needs a class or factory")
        }
        guard let elementType = viewClass as? WhiskerNativeElement.Type else {
            preconditionFailure("\(name) View class must implement WhiskerNativeElement")
        }
        let props = view.components.compactMap { $0 as? WhiskerPropComponent }
        let properties = Dictionary(uniqueKeysWithValues: props.map { ($0.name, $0) })
        let events = Set(view.components.compactMap { $0 as? WhiskerEventsComponent }.flatMap(\.names))
        register(
            WhiskerElementFactory(
                name: name,
                makeView: elementType.makeWhiskerView
            ),
            properties: properties,
            events: events
        )
    }

    private static func register(
        _ factory: WhiskerElementFactory,
        properties: [String: WhiskerPropComponent],
        events: Set<String>
    ) {
        precondition(declarations[factory.name] == nil, "duplicate Host element \(factory.name)")
        declarations[factory.name] = WhiskerDeclaredElement(factory: factory, properties: properties, events: events)
    }

    /** Match Host strings to Rust registrations and compile compact dispatch tables. */
    @discardableResult
    public static func bind(_ registrations: [WhiskerElementRegistration]) -> Bool {
        var result: [Int: WhiskerBoundElement] = [:]
        for registration in registrations {
            guard let declaration = declarations[registration.name] else {
                return false
            }
            guard registration.childPolicy.acceptsPlainText == (declaration.factory.textUpdater != nil),
                  registration.measurement == .none || registration.measurement == .text || declaration.factory.measurer != nil
            else { return false }
            let rustProps = Dictionary(uniqueKeysWithValues: registration.properties.map { ($0.name, $0) })
            guard Set(rustProps.keys) == Set(declaration.properties.keys),
                  Set(registration.events.map(\.name)) == declaration.events,
                  result[registration.elementType] == nil
            else { return false }
            let properties = Dictionary(uniqueKeysWithValues: registration.properties.map {
                ($0.id, declaration.properties[$0.name]!)
            })
            result[registration.elementType] = WhiskerBoundElement(
                registration: registration,
                factory: declaration.factory,
                properties: properties
            )
        }
        boundByType = result
        return true
    }

    public static func mount(
        _ elementType: Int,
        eventSink: @escaping WhiskerElementEventSink
    ) -> WhiskerMountedElement? {
        guard let element = boundByType[elementType] else { return nil }
        return WhiskerMountedElement(
            registration: element.registration,
            view: element.factory.makeView(),
            textUpdater: element.factory.textUpdater,
            childrenHost: element.factory.childrenHost,
            properties: element.properties,
            eventsByName: Dictionary(uniqueKeysWithValues: element.registration.events.map { ($0.name, $0) }),
            eventSink: eventSink
        )
    }

    public static func measure(
        _ elementType: Int,
        request: WhiskerMeasureRequest
    ) -> WhiskerMeasuredSize? {
        boundByType[elementType]?.factory.measurer?(request)
    }

    public static func registration(_ elementType: Int) -> WhiskerElementRegistration? {
        boundByType[elementType]?.registration
    }
}

public extension Module {
    func registerWithWhisker() {
        let definition = definitionLazy
        definition.validateElementDeclaration()
        guard let name = definition.name else { return }
        if qualifiedName == nil { qualifiedName = name }
        WhiskerModuleRegistry.register(self)
        definition.views.forEach { WhiskerElementRegistry.register($0, fallbackName: qualifiedName!) }
    }
}
