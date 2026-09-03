import WhiskerModule

private struct DecodedMember {
    let id: Int
    let name: String
    let kind: WhiskerValueKind
    let optional: Bool
}

/// Decodes and binds the Rust-provided element schema table.
enum HostElementBootstrap {
    static func bind(_ raw: WhiskerMobileBootstrap) -> Bool {
        guard raw.abi_major == UInt16(WHISKER_MOBILE_ABI_MAJOR),
              raw.protocol_major == 1,
              let base = raw.registrations else { return false }
        var registrations: [WhiskerElementRegistration] = []
        registrations.reserveCapacity(raw.registration_count)
        for index in 0..<raw.registration_count {
            let value = base.advanced(by: index).pointee
            let name = hostString(value.name)
            guard value.element_type > 0,
                  !name.isEmpty,
                  !name.contains("@"),
                  let childPolicy = decodeChildPolicy(value.child_policy),
                  let measurement = decodeMeasurement(value.measurement),
                  let properties = decodeMembers(value.properties, value.property_count),
                  let events = decodeMembers(value.events, value.event_count),
                  let commands = decodeMembers(value.commands, value.command_count),
                  membersAreUnique(properties),
                  membersAreUnique(events),
                  membersAreUnique(commands) else { return false }
            registrations.append(WhiskerElementRegistration(
                elementType: Int(value.element_type),
                name: name,
                childPolicy: childPolicy,
                measurement: measurement,
                textStyle: value.text_style != 0,
                properties: properties.map {
                    WhiskerPropertyBinding(id: $0.id, name: $0.name, value: $0.kind)
                },
                events: events.map {
                    WhiskerEventBinding(
                        id: $0.id,
                        name: $0.name,
                        detail: $0.optional ? $0.kind : nil
                    )
                },
                commands: commands.map {
                    WhiskerCommandBinding(id: $0.id, name: $0.name, arguments: $0.kind)
                }
            ))
        }
        return WhiskerElementRegistry.bind(registrations)
    }
}

private func decodeChildPolicy(_ raw: UInt8) -> WhiskerChildPolicy? {
    switch raw {
    case 0: return WhiskerChildPolicy.none
    case 1: return .elements
    case 2: return .plainText
    default: return nil
    }
}

private func decodeMeasurement(_ raw: UInt8) -> WhiskerMeasurement? {
    switch raw {
    case 0: return WhiskerMeasurement.none
    case 1: return .text
    case 2: return .replacedContent
    case 3: return .custom
    default: return nil
    }
}

private func decodeValueKind(_ raw: UInt8) -> WhiskerValueKind? {
    switch raw {
    case 0: return .null
    case 1: return .bool
    case 2: return .int
    case 3: return .float
    case 4: return .string
    case 5: return .bytes
    case 6: return .array
    case 7: return .map
    default: return nil
    }
}

private func membersAreUnique(_ members: [DecodedMember]) -> Bool {
    Set(members.map(\.id)).count == members.count
        && Set(members.map(\.name)).count == members.count
}

private func decodeMembers(
    _ base: UnsafePointer<WhiskerMobileMemberRegistration>?,
    _ count: Int
) -> [DecodedMember]? {
    if count == 0 { return [] }
    guard let base else { return nil }
    var members: [DecodedMember] = []
    members.reserveCapacity(count)
    for index in 0..<count {
        let raw = base.advanced(by: index).pointee
        let name = hostString(raw.name)
        guard raw.id > 0, !name.isEmpty, let kind = decodeValueKind(raw.value_kind) else {
            return nil
        }
        members.append(DecodedMember(
            id: Int(raw.id),
            name: name,
            kind: kind,
            optional: raw.optional_kind != 0
        ))
    }
    return members
}
