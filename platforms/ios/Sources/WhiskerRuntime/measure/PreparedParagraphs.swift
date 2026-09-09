import WhiskerModule

final class PreparedParagraphs {
    private struct Entry { weak var value: PreparedParagraph? }
    private var entries = [UInt64: Entry]()

    func insert(_ layout: WhiskerParagraphLayout, id: UInt64) -> PreparedParagraph {
        let prepared = PreparedParagraph(layout: layout, id: id, owner: self)
        entries[id] = Entry(value: prepared)
        return prepared
    }

    func layout(_ id: UInt64) -> WhiskerParagraphLayout? { entries[id]?.value?.layout }

    fileprivate func remove(_ id: UInt64, value: PreparedParagraph) {
        if entries[id]?.value === value || entries[id]?.value == nil { entries.removeValue(forKey: id) }
    }
}

final class PreparedParagraph {
    let layout: WhiskerParagraphLayout
    private let id: UInt64
    private weak var owner: PreparedParagraphs?

    init(layout: WhiskerParagraphLayout, id: UInt64, owner: PreparedParagraphs) {
        self.layout = layout
        self.id = id
        self.owner = owner
    }

    deinit { owner?.remove(id, value: self) }
}
