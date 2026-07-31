import Combine
import Foundation

@MainActor
final class BackupImportViewModel: ObservableObject {
    @Published private(set) var inspection: BackupInspectionDto?
    @Published var mode: RestoreModeDto = .merge
    @Published private(set) var choices: [String: Bool] = [:]
    @Published private(set) var isBusy = false
    @Published var errorMessage: String?
    @Published var completionMessage: String?

    private let service: any CoreServicing

    init(service: any CoreServicing) {
        self.service = service
    }

    func inspect(url: URL) async {
        guard !isBusy else { return }
        isBusy = true
        errorMessage = nil
        completionMessage = nil
        let accessing = url.startAccessingSecurityScopedResource()
        defer {
            if accessing {
                url.stopAccessingSecurityScopedResource()
            }
            isBusy = false
        }
        do {
            let inspection = try await service.inspectBackup(path: url.path)
            self.inspection = inspection
            choices = Dictionary(
                uniqueKeysWithValues: inspection.conflicts.map { ($0.key, true) }
            )
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func choice(for key: String) -> Bool {
        choices[key] ?? true
    }

    func setChoice(_ useIncoming: Bool, for key: String) {
        choices[key] = useIncoming
    }

    func apply() async -> Bool {
        guard let inspection, !isBusy else { return false }
        isBusy = true
        errorMessage = nil
        defer { isBusy = false }
        do {
            let report = try await service.applyBackup(
                inspectionID: inspection.id,
                mode: mode,
                resolutions: choices.map {
                    BackupResolutionDto(conflictKey: $0.key, useIncoming: $0.value)
                }
            )
            completionMessage = L10n.format(
                "Imported %@ records. Recovery point: %@",
                String(report.importedRecords),
                report.recoveryPath
            )
            self.inspection = nil
            choices = [:]
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    func cancel() {
        guard let inspection else { return }
        self.inspection = nil
        choices = [:]
        Task {
            try? await service.cancelBackup(inspectionID: inspection.id)
        }
    }
}
