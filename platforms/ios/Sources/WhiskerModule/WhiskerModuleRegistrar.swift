import Foundation
import UIKit

public typealias WhiskerElementEventSink = (WhiskerEventBinding, WhiskerValue) -> Void

/// Taffy available-space constraint on one measurement axis.
public enum WhiskerAvailableSpace: Equatable {
    case definite, minContent, maxContent
}

public struct WhiskerMeasureRequest {
    public let availableWidth: CGFloat?
    public let availableHeight: CGFloat?
    public let availableWidthKind: WhiskerAvailableSpace
    public let availableHeightKind: WhiskerAvailableSpace
    public let knownWidth: CGFloat?
    public let knownHeight: CGFloat?
    public let payloadVersion: UInt16
    public let payload: WhiskerValue

    public init(
        availableWidth: CGFloat?,
        availableHeight: CGFloat?,
        availableWidthKind: WhiskerAvailableSpace,
        availableHeightKind: WhiskerAvailableSpace,
        knownWidth: CGFloat?,
        knownHeight: CGFloat?,
        payloadVersion: UInt16,
        payload: WhiskerValue
    ) {
        self.availableWidth = availableWidth
        self.availableHeight = availableHeight
        self.availableWidthKind = availableWidthKind
        self.availableHeightKind = availableHeightKind
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
    public let fontFamilies: [String]
    public let fontSize: CGFloat
    public let fontWeight: Int
    public let fontStyle: WhiskerTextFontStyle
    public let lineHeight: CGFloat?
    public let letterSpacing: CGFloat
    public let fontFeatures: [WhiskerFontFeature]
    public let fontVariations: [WhiskerFontVariation]
    public let fontOpticalSizing: WhiskerFontOpticalSizing
    public let color: UIColor
    public let direction: WhiskerTextDirection
    public let alignment: WhiskerTextAlignment
    public let indent: WhiskerTextIndent
    public let wrap: Bool
    public let wordBreak: WhiskerTextWordBreak
    public let maxLines: Int
    public let overflow: WhiskerTextOverflow
    public let decoration: WhiskerTextDecoration?
    public let shadow: WhiskerTextShadow?

    public init(
        value: String,
        fontFamilies: [String] = ["system"],
        fontSize: CGFloat,
        fontWeight: Int,
        fontStyle: WhiskerTextFontStyle = .normal,
        lineHeight: CGFloat? = nil,
        letterSpacing: CGFloat = 0,
        fontFeatures: [WhiskerFontFeature] = [],
        fontVariations: [WhiskerFontVariation] = [],
        fontOpticalSizing: WhiskerFontOpticalSizing = .none,
        color: UIColor,
        direction: WhiskerTextDirection = .auto,
        alignment: WhiskerTextAlignment = .start,
        indent: WhiskerTextIndent = WhiskerTextIndent(),
        wrap: Bool = true,
        wordBreak: WhiskerTextWordBreak = .normal,
        maxLines: Int = 0,
        overflow: WhiskerTextOverflow = .clip,
        decoration: WhiskerTextDecoration? = nil,
        shadow: WhiskerTextShadow? = nil
    ) {
        self.value = value
        self.fontFamilies = fontFamilies
        self.fontSize = fontSize
        self.fontWeight = fontWeight
        self.fontStyle = fontStyle
        self.lineHeight = lineHeight
        self.letterSpacing = letterSpacing
        self.fontFeatures = fontFeatures
        self.fontVariations = fontVariations
        self.fontOpticalSizing = fontOpticalSizing
        self.color = color
        self.direction = direction
        self.alignment = alignment
        self.indent = indent
        self.wrap = wrap
        self.wordBreak = wordBreak
        self.maxLines = maxLines
        self.overflow = overflow
        self.decoration = decoration
        self.shadow = shadow
    }
}

/// Resolved inherited text style delivered independently from text content.
public struct WhiskerTextStyle {
    public let fontFamilies: [String]
    public let fontSize: CGFloat
    public let fontWeight: Int
    public let fontStyle: WhiskerTextFontStyle
    public let lineHeight: CGFloat?
    public let letterSpacing: CGFloat
    public let fontFeatures: [WhiskerFontFeature]
    public let fontVariations: [WhiskerFontVariation]
    public let fontOpticalSizing: WhiskerFontOpticalSizing
    public let color: UIColor
    public let direction: WhiskerTextDirection
    public let alignment: WhiskerTextAlignment
    public let decoration: WhiskerTextDecoration?
    public let shadow: WhiskerTextShadow?

    public init(content: WhiskerTextContent) {
        fontFamilies = content.fontFamilies
        fontSize = content.fontSize
        fontWeight = content.fontWeight
        fontStyle = content.fontStyle
        lineHeight = content.lineHeight
        letterSpacing = content.letterSpacing
        fontFeatures = content.fontFeatures
        fontVariations = content.fontVariations
        fontOpticalSizing = content.fontOpticalSizing
        color = content.color
        direction = content.direction
        alignment = content.alignment
        decoration = content.decoration
        shadow = content.shadow
    }
}

public enum WhiskerTextFontStyle: Equatable {
    case normal
    case italic
    case oblique
}

public struct WhiskerFontFeature: Equatable {
    public let tag: String
    public let value: UInt32

    public init(tag: String, value: UInt32) {
        self.tag = tag
        self.value = value
    }
}

public struct WhiskerFontVariation: Equatable {
    public let tag: String
    public let value: CGFloat

    public init(tag: String, value: CGFloat) {
        self.tag = tag
        self.value = value
    }
}

public enum WhiskerFontOpticalSizing: Equatable { case auto, none }

public enum WhiskerTextDirection: Equatable { case auto, leftToRight, rightToLeft }
public enum WhiskerTextAlignment: Equatable { case start, end, left, right, center }
public enum WhiskerTextWordBreak: Equatable { case normal, breakAll, keepAll }
public enum WhiskerTextOverflow: Equatable { case clip, ellipsis }

/** First-line indentation; percentage is relative to the final Text width. */
public struct WhiskerTextIndent: Equatable {
    public let logicalPixels: CGFloat
    public let percentage: CGFloat

    public init(logicalPixels: CGFloat = 0, percentage: CGFloat = 0) {
        self.logicalPixels = logicalPixels
        self.percentage = percentage
    }

    public func resolve(width: CGFloat) -> CGFloat {
        logicalPixels + width * percentage / 100
    }
}

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
    private let eventSource: WhiskerEventSource?
    private let textUpdater: ((UIView, WhiskerTextContent) -> Void)?
    private let textStyleUpdater: ((UIView, WhiskerTextStyle) -> Void)?
    private let childrenHostProvider: ((UIView) -> UIView)?
    private let properties: [Int: WhiskerPropComponent]
    private let commands: [Int: WhiskerCommandComponent]
    private let eventsByName: [String: WhiskerEventBinding]
    private var eventSink: WhiskerElementEventSink
    private var eventMask: UInt64 = 0

    fileprivate init(
        registration: WhiskerElementRegistration,
        view: UIView,
        textUpdater: ((UIView, WhiskerTextContent) -> Void)?,
        textStyleUpdater: ((UIView, WhiskerTextStyle) -> Void)?,
        childrenHost: ((UIView) -> UIView)?,
        properties: [Int: WhiskerPropComponent],
        commands: [Int: WhiskerCommandComponent],
        eventsByName: [String: WhiskerEventBinding],
        eventSink: @escaping WhiskerElementEventSink
    ) {
        self.registration = registration
        self.view = view
        self.eventSource = view as? WhiskerEventSource
        self.textUpdater = textUpdater
        self.textStyleUpdater = textStyleUpdater
        self.childrenHostProvider = childrenHost
        self.properties = properties
        self.commands = commands
        self.eventsByName = eventsByName
        self.eventSink = eventSink
        installEventSink()
    }

    private func installEventSink() {
        eventSource?.installWhiskerEventSink { [weak self] name, detail in
            guard let self, let event = self.eventsByName[name] else { return }
            let bit = UInt64(1) << UInt64(event.id - 1)
            if self.eventMask & bit != 0 { self.eventSink(event, detail) }
        }
    }

    public func setProperty(_ id: Int, value: WhiskerValue) { properties[id]?.setter(view, value) }

    /// Clear is a distinct protocol operation; it is not converted to `.null`.
    public func clearProperty(_ id: Int) { properties[id]?.clearer(view) }

    public func invokeCommand(_ id: Int, parameters: WhiskerValue) {
        commands[id]?.handler(view, parameters)
    }

    public func setEventMask(_ mask: UInt64) { eventMask = mask }

    @discardableResult
    public func setText(_ content: WhiskerTextContent) -> Bool {
        guard let textUpdater else { return false }
        textUpdater(view, content)
        return true
    }

    @discardableResult
    public func setTextStyle(_ style: WhiskerTextStyle) -> Bool {
        guard let textStyleUpdater else { return false }
        textStyleUpdater(view, style)
        return true
    }

    public func childrenHost() -> UIView? { childrenHostProvider?(view) }

    /** Resets protocol-owned state before a built-in presentation is reused. */
    public func prepareForReuse(eventSink: @escaping WhiskerElementEventSink) {
        properties.values.forEach { $0.clearer(view) }
        view.isHidden = false
        eventMask = 0
        self.eventSink = eventSink
        installEventSink()
    }

    public func dispose() { eventSource?.installWhiskerEventSink(nil) }
}

/** Host-owned declaration. It contains names and behavior, never Rust IDs. */
public struct WhiskerElementFactory {
    public let name: String
    fileprivate let textUpdater: ((UIView, WhiskerTextContent) -> Void)?
    fileprivate let textStyleUpdater: ((UIView, WhiskerTextStyle) -> Void)?
    fileprivate let childrenHost: ((UIView) -> UIView)?
    fileprivate let measurer: ((WhiskerMeasureRequest) -> WhiskerMeasuredSize?)?
    fileprivate let makeView: () -> UIView

    public init(
        name: String,
        textUpdater: ((UIView, WhiskerTextContent) -> Void)? = nil,
        textStyleUpdater: ((UIView, WhiskerTextStyle) -> Void)? = nil,
        childrenHost: ((UIView) -> UIView)? = nil,
        measurer: ((WhiskerMeasureRequest) -> WhiskerMeasuredSize?)? = nil,
        makeView: @escaping () -> UIView
    ) {
        precondition(!name.isEmpty && !name.contains("@"))
        self.name = name
        self.textUpdater = textUpdater
        self.textStyleUpdater = textStyleUpdater
        self.childrenHost = childrenHost
        self.measurer = measurer
        self.makeView = makeView
    }

    fileprivate func withTextStyleUpdater(
        _ updater: ((UIView, WhiskerTextStyle) -> Void)?
    ) -> WhiskerElementFactory {
        WhiskerElementFactory(
            name: name,
            textUpdater: textUpdater,
            textStyleUpdater: updater ?? textStyleUpdater,
            childrenHost: childrenHost,
            measurer: measurer,
            makeView: makeView
        )
    }

    fileprivate func withMeasurer(
        _ provider: ((WhiskerMeasureRequest) -> WhiskerMeasuredSize?)?
    ) -> WhiskerElementFactory {
        WhiskerElementFactory(
            name: name,
            textUpdater: textUpdater,
            textStyleUpdater: textStyleUpdater,
            childrenHost: childrenHost,
            measurer: provider ?? measurer,
            makeView: makeView
        )
    }
}

private struct WhiskerDeclaredElement {
    let factory: WhiskerElementFactory
    let properties: [String: WhiskerPropComponent]
    let events: Set<String>
    let commands: [String: WhiskerCommandComponent]
}

private struct WhiskerBoundElement {
    let registration: WhiskerElementRegistration
    let factory: WhiskerElementFactory
    let properties: [Int: WhiskerPropComponent]
    let commands: [Int: WhiskerCommandComponent]
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
        register(factory, properties: [:], events: [], commands: [:])
    }

    fileprivate static func register(_ view: WhiskerViewComponent, fallbackName: String) {
        let name = view.elementName ?? fallbackName
        if let factory = view.factory {
            let props = view.components.compactMap { $0 as? WhiskerPropComponent }
            let properties = Dictionary(uniqueKeysWithValues: props.map { ($0.name, $0) })
            let events = Set(view.components.compactMap { $0 as? WhiskerEventsComponent }.flatMap(\.names))
            let commands = Dictionary(uniqueKeysWithValues: view.components.compactMap { $0 as? WhiskerCommandComponent }.map { ($0.name, $0) })
            let textStyle = view.components.compactMap { $0 as? WhiskerTextStyleComponent }.first
            let measurement = view.components.compactMap { $0 as? WhiskerMeasurementComponent }.first
            register(
                factory.withTextStyleUpdater(textStyle?.handler).withMeasurer(measurement?.handler),
                properties: properties,
                events: events,
                commands: commands
            )
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
        let commands = Dictionary(uniqueKeysWithValues: view.components.compactMap { $0 as? WhiskerCommandComponent }.map { ($0.name, $0) })
        let textStyle = view.components.compactMap { $0 as? WhiskerTextStyleComponent }.first
        let measurement = view.components.compactMap { $0 as? WhiskerMeasurementComponent }.first
        register(
            WhiskerElementFactory(
                name: name,
                textStyleUpdater: textStyle?.handler,
                measurer: measurement?.handler,
                makeView: elementType.makeWhiskerView
            ),
            properties: properties,
            events: events,
            commands: commands
        )
    }

    private static func register(
        _ factory: WhiskerElementFactory,
        properties: [String: WhiskerPropComponent],
        events: Set<String>,
        commands: [String: WhiskerCommandComponent]
    ) {
        precondition(declarations[factory.name] == nil, "duplicate Host element \(factory.name)")
        declarations[factory.name] = WhiskerDeclaredElement(factory: factory, properties: properties, events: events, commands: commands)
    }

    /** Match Host strings to Rust registrations and compile compact dispatch tables. */
    @discardableResult
    public static func bind(_ registrations: [WhiskerElementRegistration]) -> Bool {
        var result: [Int: WhiskerBoundElement] = [:]
        for registration in registrations {
            guard let declaration = declarations[registration.name] else {
                return false
            }
            let needsHostMeasurer = registration.measurement == .replacedContent
                || registration.measurement == .custom
            guard registration.childPolicy.acceptsPlainText == (declaration.factory.textUpdater != nil),
                  registration.textStyle == (declaration.factory.textStyleUpdater != nil),
                  needsHostMeasurer == (declaration.factory.measurer != nil)
            else { return false }
            let rustProps = Dictionary(uniqueKeysWithValues: registration.properties.map { ($0.name, $0) })
            guard Set(rustProps.keys) == Set(declaration.properties.keys),
                  Set(registration.events.map(\.name)) == declaration.events,
                  Set(registration.commands.map(\.name)) == Set(declaration.commands.keys),
                  result[registration.elementType] == nil
            else { return false }
            let properties = Dictionary(uniqueKeysWithValues: registration.properties.map {
                ($0.id, declaration.properties[$0.name]!)
            })
            let commands = Dictionary(uniqueKeysWithValues: registration.commands.map {
                ($0.id, declaration.commands[$0.name]!)
            })
            result[registration.elementType] = WhiskerBoundElement(
                registration: registration,
                factory: declaration.factory,
                properties: properties,
                commands: commands
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
            textStyleUpdater: element.factory.textStyleUpdater,
            childrenHost: element.factory.childrenHost,
            properties: element.properties,
            commands: element.commands,
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

/// Single bootstrap boundary for independently compiled Host modules.
public enum WhiskerModuleKernel {
    private static let lock = NSLock()
    private static var installedNames: Set<String> = []

    /// Validates and installs one complete service + element declaration.
    public static func install(_ module: Module) {
        let definition = module.definitionLazy
        definition.validateElementDeclaration()
        guard let name = definition.name else {
            preconditionFailure("ModuleDefinition requires Name")
        }
        if module.qualifiedName == nil { module.qualifiedName = name }
        let qualifiedName = module.qualifiedName!
        lock.lock()
        let inserted = installedNames.insert(qualifiedName).inserted
        lock.unlock()
        precondition(inserted, "module already installed: \(qualifiedName)")
        WhiskerModuleRegistry.register(module)
        definition.views.forEach { WhiskerElementRegistry.register($0, fallbackName: qualifiedName) }
    }
}
