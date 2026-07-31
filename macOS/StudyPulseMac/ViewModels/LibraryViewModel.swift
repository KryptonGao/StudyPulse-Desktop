import Combine
import Foundation

@MainActor
final class LibraryViewModel: ObservableObject {
    @Published private(set) var files: [FileEntryDto] = []
    @Published private(set) var matches: [SearchMatchDto] = []
    @Published var selectedPath: String?
    @Published var query = ""
    @Published private(set) var isLoading = false
    @Published var errorMessage: String?
    var onFilesChanged: (@MainActor ([FileEntryDto]) -> Void)?

    private let service: any CoreServicing

    init(service: any CoreServicing) {
        self.service = service
    }

    var selectedFile: FileEntryDto? {
        guard let selectedPath else { return nil }
        return files.first { $0.relativePath == selectedPath }
    }

    var selectedMatches: [SearchMatchDto] {
        guard let selectedPath else { return [] }
        return matches.filter { $0.relativePath == selectedPath }
    }

    func refresh() async {
        guard !isLoading else { return }
        isLoading = true
        defer { isLoading = false }
        do {
            files = try await service.listLibraryFiles()
            if let selectedPath, !files.contains(where: { $0.relativePath == selectedPath }) {
                self.selectedPath = nil
            }
            onFilesChanged?(files)
            if !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                matches = try await service.searchLibrary(query: query)
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func search() {
        let value = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else {
            matches = []
            return
        }
        Task {
            isLoading = true
            defer { isLoading = false }
            do {
                matches = try await service.searchLibrary(query: value)
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func reset() {
        files = []
        matches = []
        selectedPath = nil
        query = ""
        errorMessage = nil
        onFilesChanged?([])
    }
}
