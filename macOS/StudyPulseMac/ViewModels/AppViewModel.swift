import Combine
import Foundation

@MainActor
final class AppViewModel: ObservableObject {
    @Published var destination: AppDestination = .today
    @Published var isShowingBackupImport = false

    let workspace: WorkspaceViewModel
    let agent: AgentViewModel
    let tasks: TasksViewModel
    let p0: P0ViewModel
    let library: LibraryViewModel
    let backup: BackupImportViewModel

    private let panel: any WorkspacePanelServing

    init(
        service: any CoreServicing = LiveCoreService(),
        panel: any WorkspacePanelServing = WorkspacePanelService()
    ) {
        workspace = WorkspaceViewModel(service: service)
        agent = AgentViewModel(service: service)
        tasks = TasksViewModel(service: service)
        p0 = P0ViewModel(service: service)
        library = LibraryViewModel(service: service)
        backup = BackupImportViewModel(service: service)
        self.panel = panel
        agent.onTasksChanged = { [weak tasks] in
            await tasks?.refresh()
        }
        agent.onLibraryChanged = { [weak library] in
            await library?.refresh()
        }
        library.onFilesChanged = { [weak agent] files in
            agent?.updateAvailableSources(files)
        }

        Task { [weak self] in
            guard let self else { return }
            await agent.restoreAIConfiguration()
            guard await workspace.restorePreviousWorkspace() else { return }
            await refreshWorkspaceViews()
        }
    }

    func createWorkspace() {
        guard let url = panel.chooseWorkspaceToCreate() else { return }
        Task {
            if await workspace.create(at: url) {
                await workspaceDidChange()
            }
        }
    }

    func openWorkspace() {
        guard let url = panel.chooseWorkspaceToOpen() else { return }
        Task {
            if await workspace.open(at: url) {
                await workspaceDidChange()
            }
        }
    }

    func importBackup() {
        guard workspace.workspace != nil, let url = panel.chooseBackup() else { return }
        isShowingBackupImport = true
        Task {
            await backup.inspect(url: url)
        }
    }

    func exportBackup() {
        guard workspace.workspace != nil, let url = panel.chooseBackupExport() else { return }
        Task { await p0.exportBackup(to: url) }
    }

    func chooseAgentSourceFiles() {
        let urls = panel.chooseAgentSourceFiles()
        guard !urls.isEmpty else { return }
        Task {
            await agent.importSourceFiles(urls)
        }
    }

    func closeWorkspace() {
        Task {
            await workspace.close()
            agent.resetForWorkspace()
            tasks.reset()
            p0.reset()
            library.reset()
        }
    }

    func refreshWorkspaceViews() async {
        async let taskRefresh: Void = tasks.refresh()
        async let libraryRefresh: Void = library.refresh()
        async let p0Refresh: Void = p0.refresh()
        _ = await (taskRefresh, libraryRefresh, p0Refresh)
        if let workspace = workspace.workspace {
            await agent.configure(workspaceID: workspace.id, files: library.files)
        }
    }

    func didApplyBackup() async {
        isShowingBackupImport = false
        await refreshWorkspaceViews()
    }

    private func workspaceDidChange() async {
        destination = .agent
        agent.resetForWorkspace()
        p0.reset()
        await refreshWorkspaceViews()
    }
}
