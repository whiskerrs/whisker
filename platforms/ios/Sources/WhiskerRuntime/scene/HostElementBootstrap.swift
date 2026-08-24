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
        let registrations = (0..<raw.registration_count).map { index -> WhiskerElementRegistration in
            let value = base.advanced(by: index).pointee
            return WhiskerElementRegistration(
                elementType: Int(value.element_type),
                name: hostString(value.name),
                childPolicy: [WhiskerChildPolicy.none, .elements, .plainText][Int(value.child_policy)],
                measurement: [WhiskerMeasurement.none, .text, .replacedContent, .custom][Int(value.measurement)],
                properties: decodeMembers(value.properties, value.property_count).map {
                    WhiskerPropertyBinding(id: $0.id, name: $0.name, value: $0.kind)
                },
                events: decodeMembers(value.events, value.event_count).map {
                    WhiskerEventBinding(
                        id: $0.id,
                        name: $0.name,
                        detail: $0.optional ? $0.kind : nil
                    )
                },
                commands: decodeMembers(value.commands, value.command_count).map {
                    WhiskerCommandBinding(id: $0.id, name: $0.name, arguments: $0.kind)
                }
            )
        }
        return WhiskerElementRegistry.bind(registrations)
    }
}

private func decodeMembers(
    _ base: UnsafePointer<WhiskerMobileMemberRegistration>?,
    _ count: Int
) -> [DecodedMember] {
    guard let base else { return [] }
    return (0..<count).map { index in
        let raw = base.advanced(by: index).pointee
        let kinds: [WhiskerValueKind] = [
            .null, .bool, .int, .float, .string, .bytes, .array, .map,
        ]
        return DecodedMember(
            id: Int(raw.id),
            name: hostString(raw.name),
            kind: kinds[Int(raw.value_kind)],
            optional: raw.optional_kind != 0
        )
    }
}
