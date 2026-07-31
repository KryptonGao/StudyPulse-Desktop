import AppKit
import Foundation
import UniformTypeIdentifiers

@MainActor
protocol WorkspacePanelServing {
    func chooseWorkspaceToCreate() -> URL?
    func chooseWorkspaceToOpen() -> URL?
    func chooseBackup() -> URL?
    func chooseBackupExport() -> URL?
    func chooseAgentSourceFiles() -> [URL]
}

@MainActor
struct WorkspacePanelService: WorkspacePanelServing {
    func chooseWorkspaceToCreate() -> URL? {
        let panel = NSSavePanel()
        panel.title = L10n.string("Create StudyPulse Workspace")
        panel.prompt = L10n.string("Create")
        panel.nameFieldStringValue = "StudyPulseWorkspace"
        panel.canCreateDirectories = true
        panel.isExtensionHidden = true
        return panel.runModal() == .OK ? panel.url : nil
    }

    func chooseWorkspaceToOpen() -> URL? {
        let panel = NSOpenPanel()
        panel.title = L10n.string("Open StudyPulse Workspace")
        panel.prompt = L10n.string("Open")
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        return panel.runModal() == .OK ? panel.url : nil
    }

    func chooseBackup() -> URL? {
        let panel = NSOpenPanel()
        panel.title = L10n.string("Import StudyPulse Backup")
        panel.prompt = L10n.string("Inspect")
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [.data]
        return panel.runModal() == .OK ? panel.url : nil
    }

    func chooseBackupExport() -> URL? {
        let panel = NSSavePanel()
        panel.title = L10n.string("Export StudyPulse Backup")
        panel.prompt = L10n.string("Export")
        panel.nameFieldStringValue = "StudyPulse-Backup.studypulsebackup"
        panel.isExtensionHidden = false
        panel.allowedContentTypes = [.data]
        return panel.runModal() == .OK ? panel.url : nil
    }

    func chooseAgentSourceFiles() -> [URL] {
        let panel = NSOpenPanel()
        panel.title = L10n.string("Add Notebook Sources")
        panel.prompt = L10n.string("Add")
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = true
        panel.allowedContentTypes = [.text]
        return panel.runModal() == .OK ? panel.urls : []
    }
}

@MainActor
final class SecurityScopedWorkspaceAccess {
    private(set) var url: URL?
    private var isAccessing = false

    deinit {
        if isAccessing {
            url?.stopAccessingSecurityScopedResource()
        }
    }

    func retainAccess(to selectedURL: URL) {
        release()
        url = selectedURL
        isAccessing = selectedURL.startAccessingSecurityScopedResource()
        if let bookmark = try? selectedURL.bookmarkData(
            options: .withSecurityScope,
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        ) {
            UserDefaults.standard.set(bookmark, forKey: "StudyPulseWorkspaceBookmark")
        }
    }

    func restore() -> URL? {
        guard let data = UserDefaults.standard.data(forKey: "StudyPulseWorkspaceBookmark") else {
            return nil
        }
        var stale = false
        guard let restored = try? URL(
            resolvingBookmarkData: data,
            options: .withSecurityScope,
            relativeTo: nil,
            bookmarkDataIsStale: &stale
        ) else {
            return nil
        }
        retainAccess(to: restored)
        return restored
    }

    func release() {
        if isAccessing {
            url?.stopAccessingSecurityScopedResource()
        }
        isAccessing = false
        url = nil
    }
}
