import SwiftUI

/// Fits a finite set of cells into each viewport, then pages horizontally.
struct LibraryPager<Item, ID: Hashable, Cell: View>: View {
    let items: [Item]
    let id: KeyPath<Item, ID>
    let minimumCellSize: CGSize
    let maximumColumns: Int
    private let cell: (Item) -> Cell

    @State private var visiblePageID: ID?

    init(
        items: [Item], id: KeyPath<Item, ID>, minimumCellSize: CGSize,
        maximumColumns: Int, @ViewBuilder cell: @escaping (Item) -> Cell
    ) {
        self.items = items
        self.id = id
        self.minimumCellSize = minimumCellSize
        self.maximumColumns = maximumColumns
        self.cell = cell
    }

    var body: some View {
        GeometryReader { geometry in
            let gap: CGFloat = 12
            let columns = min(
                maximumColumns,
                max(1, Int((geometry.size.width + gap) / (minimumCellSize.width + gap))))
            let contentHeight = max(0, geometry.size.height - 56)
            let cellHeight = min(minimumCellSize.height, contentHeight)
            let rows = min(5, max(1, Int((contentHeight + gap) / (minimumCellSize.height + gap))))
            let pages = makePages(capacity: columns * rows)
            let current = pages.firstIndex { $0.id == visiblePageID } ?? 0

            VStack(spacing: 12) {
                TabView(selection: $visiblePageID) {
                    ForEach(pages) { page in
                        VStack(spacing: 0) {
                            LazyVGrid(
                                columns: Array(
                                    repeating: GridItem(.flexible(), spacing: gap), count: columns),
                                spacing: gap
                            ) {
                                ForEach(page.items, id: id) { item in
                                    cell(item)
                                        .frame(height: cellHeight)
                                }
                            }
                            Spacer(minLength: 0)
                        }
                        .tag(Optional(page.id))
                    }
                }
                .tabViewStyle(.page(indexDisplayMode: .never))

                if pages.count > 1 {
                    HStack(spacing: 24) {
                        Button {
                            visiblePageID = pages[max(0, current - 1)].id
                        } label: {
                            Image(systemName: "chevron.left").frame(width: 44, height: 44)
                        }
                        .disabled(current == 0)
                        .accessibilityLabel("Previous page")

                        Text("\(current + 1) of \(pages.count)")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                            .accessibilityLabel("Page \(current + 1) of \(pages.count)")

                        Button {
                            visiblePageID = pages[min(pages.count - 1, current + 1)].id
                        } label: {
                            Image(systemName: "chevron.right").frame(width: 44, height: 44)
                        }
                        .disabled(current == pages.count - 1)
                        .accessibilityLabel("Next page")
                    }
                    .buttonStyle(.plain)
                }
            }
            .onAppear { alignSelection(in: pages) }
            .onChange(of: pages.map(\.id)) { _, _ in alignSelection(in: pages) }
        }
    }

    private func makePages(capacity: Int) -> [LibraryItemPage<Item, ID>] {
        stride(from: 0, to: items.count, by: capacity).map { start in
            let slice = Array(items[start..<min(start + capacity, items.count)])
            return LibraryItemPage(id: items[start][keyPath: id], items: slice)
        }
    }

    private func alignSelection(in pages: [LibraryItemPage<Item, ID>]) {
        // Keep the selected item visible when rotation changes the number of cells per page.
        if let selected = visiblePageID,
            let containing = pages.first(where: {
                $0.items.contains { $0[keyPath: id] == selected }
            })
        {
            visiblePageID = containing.id
        } else {
            visiblePageID = pages.first?.id
        }
    }
}

private struct LibraryItemPage<Item, ID: Hashable>: Identifiable {
    let id: ID
    let items: [Item]
}
