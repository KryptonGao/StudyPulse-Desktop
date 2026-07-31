import Combine
import Foundation

@MainActor
final class WorkspaceViewModel: ObservableObject {
    @Published private(set) var workspace: WorkspaceDto?
    @Published private(set) var isBusy = false
    @Published var errorMessage: String?

    private let service: any CoreServicing
    private let access = SecurityScopedWorkspaceAccess()

    init(service: any CoreServicing) {
        self.service = service
    }

    func create(at url: URL) async -> Bool {
        await openOperation(url: url, create: true)
    }

    func open(at url: URL) async -> Bool {
        await openOperation(url: url, create: false)
    }

    func restorePreviousWorkspace() async -> Bool {
        guard let url = access.restore() else { return false }
        return await openOperation(url: url, create: false, retainAccess: false)
    }

    func close() async {
        do {
            try await service.closeWorkspace()
            workspace = nil
            access.release()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func openOperation(
        url: URL,
        create: Bool,
        retainAccess: Bool = true
    ) async -> Bool {
        guard !isBusy else { return false }
        isBusy = true
        errorMessage = nil
        defer { isBusy = false }
        if retainAccess {
            access.retainAccess(to: url)
        }
        do {
            workspace = if create {
                try await service.createWorkspace(path: url.path)
            } else {
                try await service.openWorkspace(path: url.path)
            }
            return true
        } catch {
            errorMessage = error.localizedDescription
            if retainAccess {
                access.release()
            }
            return false
        }
    }
}
