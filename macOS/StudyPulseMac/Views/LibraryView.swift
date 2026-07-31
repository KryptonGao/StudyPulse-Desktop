import Foundation
import SwiftUI

struct LibraryView: View {
    @ObservedObject var viewModel: LibraryViewModel

    var body: some View {
        libraryContent
            .searchable(text: $viewModel.query, prompt: "Search Documents and Notes")
            .onSubmit(of: .search) {
                viewModel.search()
            }
            .onChange(of: viewModel.query) {
                if viewModel.query.isEmpty {
                    viewModel.search()
                }
            }
            .overlay {
                if viewModel.isLoading {
                    ProgressView()
                        .padding(12)
                        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
                }
            }
            .toolbar {
                Button {
                    Task { await viewModel.refresh() }
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(viewModel.isLoading)
            }
    }

    @ViewBuilder
    private var libraryContent: some View {
        Group {
            if !viewModel.matches.isEmpty {
                List {
                    ForEach(viewModel.matches.indices, id: \.self) { index in
                        let match = viewModel.matches[index]
                        Button {
                            viewModel.selectedPath = match.relativePath
                        } label: {
                            VStack(alignment: .leading, spacing: 5) {
                                Label(match.relativePath, systemImage: "doc.text")
                                    .font(.subheadline.weight(.medium))
                                Text(match.snippet)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(3)
                                if let line = match.lineNumber {
                                    Text(L10n.format("Line %@", String(line)))
                                        .font(.caption2)
                                        .foregroundStyle(.tertiary)
                                }
                            }
                        }
                        .buttonStyle(.plain)
                        .padding(.vertical, 4)
                        .listRowBackground(viewModel.selectedPath == match.relativePath ? Color.accentColor.opacity(0.14) : Color.clear)
                    }
                }
            } else if viewModel.files.isEmpty {
                ContentUnavailableView(
                    "Library is empty",
                    systemImage: "books.vertical",
                    description: Text("Add text files to Documents or Notes in the Workspace.")
                )
            } else {
                List {
                    ForEach(viewModel.files.indices, id: \.self) { index in
                        let entry = viewModel.files[index]
                        Button {
                            viewModel.selectedPath = entry.relativePath
                        } label: {
                            Label {
                                HStack {
                                    Text(entry.relativePath)
                                    Spacer()
                                    if !entry.isDirectory {
                                        Text(ByteCountFormatter.string(
                                            fromByteCount: Int64(entry.sizeBytes),
                                            countStyle: .file
                                        ))
                                        .foregroundStyle(.secondary)
                                    }
                                }
                            } icon: {
                                Image(systemName: entry.isDirectory ? "folder" : "doc")
                            }
                        }
                        .buttonStyle(.plain)
                        .padding(.vertical, 4)
                        .listRowBackground(viewModel.selectedPath == entry.relativePath ? Color.accentColor.opacity(0.14) : Color.clear)
                    }
                }
            }
        }
    }
}
