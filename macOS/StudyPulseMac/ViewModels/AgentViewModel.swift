import Combine
import Foundation

@MainActor
final class AgentViewModel: ObservableObject {
    @Published var goal = ""
    @Published private(set) var answer = ""
    @Published private(set) var events: [AgentEventDto] = []
    @Published private(set) var notebooks: [AgentNotebook] = []
    @Published private(set) var selectedNotebookID: UUID?
    @Published private(set) var availableSources: [FileEntryDto] = []
    @Published private(set) var isRunning = false
    @Published private(set) var status: RunStatusDto?
    @Published var mode: AgentModeDto = .chat
    @Published private(set) var capabilities: [CapabilityManifestDto] = []
    @Published private(set) var cloudAccount: CloudAccountDto?
    @Published private(set) var byokConfig: ByokConfigDto?
    @Published private(set) var hasSavedBYOK = false
    @Published private(set) var isAuthenticating = false
    @Published private(set) var isConfiguringBYOK = false
    @Published private(set) var isImportingSources = false
    @Published var pendingConfirmation: PendingConfirmation?
    @Published var pendingInput: PendingAgentInput?
    @Published var errorMessage: String?
    @Published var isShowingSourcePicker = false

    var onTasksChanged: (@MainActor () async -> Void)?
    var onLibraryChanged: (@MainActor () async -> Void)?
    var isCloudConnected: Bool { cloudAccount != nil }
    var isBYOKConnected: Bool { byokConfig != nil }
    var isAIConfigured: Bool { isCloudConnected || isBYOKConnected }
    var currentActivity: String? {
        if pendingConfirmation != nil {
            return L10n.string("Waiting for your permission")
        }
        if pendingInput != nil {
            return L10n.string("Waiting for your answer")
        }
        guard isRunning else { return nil }

        if let requested = events.last(where: { $0.kind == .toolRequested }) {
            let completed = events.contains {
                $0.kind == .toolCompleted && $0.toolCallId == requested.toolCallId
            }
            if !completed {
                return L10n.format(
                    "Using %@",
                    requested.toolDisplayName ?? requested.toolName ?? L10n.string("tool")
                )
            }
        }
        if let stage = events.last(where: {
            $0.kind == .stageStarted || $0.kind == .stageProgress
        })?.stage {
            return L10n.format("Working on %@", stage.capitalized)
        }
        return status?.displayName ?? L10n.string("Thinking")
    }
    var selectedNotebook: AgentNotebook? {
        guard let selectedNotebookID else { return nil }
        return notebooks.first { $0.id == selectedNotebookID }
    }
    var selectedMessages: [AgentChatMessage] { selectedNotebook?.messages ?? [] }
    var selectedSourcePaths: [String] { selectedNotebook?.sourcePaths ?? [] }
    var selectedSources: [FileEntryDto] {
        let selected = Set(selectedSourcePaths)
        return availableSources
            .filter { !$0.isDirectory && selected.contains($0.relativePath) }
            .sorted { $0.relativePath.localizedStandardCompare($1.relativePath) == .orderedAscending }
    }
    var selectableSources: [FileEntryDto] {
        availableSources
            .filter { !$0.isDirectory }
            .sorted { $0.relativePath.localizedStandardCompare($1.relativePath) == .orderedAscending }
    }

    private let service: any CoreServicing
    private let authPresenter: any CloudAuthPresenting
    private let credentialStore: any CloudCredentialStoring
    private let byokCredentialStore: any BYOKCredentialStoring
    private var workspaceID: String?
    private var runID: String?
    private var eventTask: Task<Void, Never>?
    private var activeAssistantMessageID: UUID?
    private let providerDefaultsKey = "StudyPulse.agentProvider"

    init(
        service: any CoreServicing,
        authPresenter: any CloudAuthPresenting = CloudWebAuthPresenter(),
        credentialStore: any CloudCredentialStoring = CloudCredentialStore(),
        byokCredentialStore: any BYOKCredentialStoring = BYOKCredentialStore()
    ) {
        self.service = service
        self.authPresenter = authPresenter
        self.credentialStore = credentialStore
        self.byokCredentialStore = byokCredentialStore
    }

    deinit {
        eventTask?.cancel()
    }

    func start() {
        let trimmedGoal = goal.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedGoal.isEmpty, !isRunning, selectedNotebook != nil else { return }
        guard isAIConfigured else {
            errorMessage = L10n.string("Connect Cloud AI or configure BYOK before running the Agent.")
            return
        }
        let history = selectedMessages
        answer = ""
        events = []
        errorMessage = nil
        isRunning = true
        status = .started
        activeAssistantMessageID = nil
        updateSelectedNotebook {
            $0.messages.append(AgentChatMessage(role: .user, content: trimmedGoal))
            $0.lastGoal = trimmedGoal
            $0.lastAnswer = ""
        }
        goal = ""
        let sourcePaths = selectedSourcePaths

        eventTask?.cancel()
        eventTask = Task { [weak self] in
            guard let self else { return }
            do {
                let runID = try await service.startAgent(
                    mode: self.mode,
                    goal: trimmedGoal,
                    sourcePaths: sourcePaths,
                    history: history.map(\.dto)
                )
                self.runID = runID
                for try await event in service.agentEvents(runID: runID) {
                    await self.consume(event)
                }
            } catch is CancellationError {
                return
            } catch {
                errorMessage = error.localizedDescription
                isRunning = false
                activeAssistantMessageID = nil
                updateSelectedNotebook { _ in }
            }
        }
    }

    func cancel() {
        guard let runID, isRunning else { return }
        Task {
            do {
                try await service.cancelAgent(runID: runID)
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func resolveConfirmation(allow: Bool) {
        guard let pendingConfirmation else { return }
        self.pendingConfirmation = nil
        Task {
            do {
                try await service.submitConfirmation(
                    runID: pendingConfirmation.runID,
                    confirmationID: pendingConfirmation.id,
                    decision: allow ? .allow : .deny
                )
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func resolveInput(answer: String) {
        guard let pendingInput else { return }
        self.pendingInput = nil
        let answerJSON = (try? JSONSerialization.data(withJSONObject: ["answer": answer]))
            .flatMap { String(data: $0, encoding: .utf8) } ?? "{\"answer\":\"\"}"
        Task {
            do {
                try await service.submitAgentInput(
                    runID: pendingInput.runID,
                    inputID: pendingInput.id,
                    answerJSON: answerJSON
                )
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func resetForWorkspace() {
        eventTask?.cancel()
        workspaceID = nil
        runID = nil
        activeAssistantMessageID = nil
        answer = ""
        events = []
        notebooks = []
        selectedNotebookID = nil
        availableSources = []
        isRunning = false
        status = nil
        pendingConfirmation = nil
        pendingInput = nil
        errorMessage = nil
    }

    func configure(workspaceID: String, files: [FileEntryDto]) async {
        availableSources = files
        guard self.workspaceID != workspaceID else { return }
        eventTask?.cancel()
        runID = nil
        activeAssistantMessageID = nil
        isRunning = false
        status = nil
        events = []
        pendingConfirmation = nil
        pendingInput = nil
        errorMessage = nil
        self.workspaceID = workspaceID

        do {
            notebooks = try await service.getAgentNotebooks().map(AgentNotebook.init(dto:))
            capabilities = try await service.listAgentCapabilities()
        } catch {
            errorMessage = error.localizedDescription
            notebooks = []
        }
        updateAvailableSources(files)
        if notebooks.isEmpty {
            notebooks = [AgentNotebook(title: L10n.string("Untitled Notebook"))]
        }
        selectedNotebookID = notebooks.first?.id
        restoreSelectedNotebook()
        persistNotebooks()
    }

    func updateAvailableSources(_ files: [FileEntryDto]) {
        availableSources = files
        let availablePaths = Set(files.lazy.filter { !$0.isDirectory }.map(\.relativePath))
        var didChange = false
        for index in notebooks.indices {
            let current = notebooks[index].sourcePaths
            let existing = current.filter { availablePaths.contains($0) }
            if existing != current {
                notebooks[index].sourcePaths = existing
                notebooks[index].updatedAt = .now
                didChange = true
            }
        }
        if didChange {
            persistNotebooks()
        }
    }

    func createNotebook() {
        guard !isRunning else { return }
        let usedTitles = Set(notebooks.map(\.title))
        var index = notebooks.count + 1
        var title = L10n.format("Untitled Notebook %@", String(index))
        while usedTitles.contains(title) {
            index += 1
            title = L10n.format("Untitled Notebook %@", String(index))
        }
        let notebook = AgentNotebook(title: title)
        notebooks.insert(notebook, at: 0)
        selectedNotebookID = notebook.id
        restoreSelectedNotebook()
        persistNotebooks()
    }

    func selectNotebook(_ id: UUID) {
        guard !isRunning, notebooks.contains(where: { $0.id == id }) else { return }
        selectedNotebookID = id
        restoreSelectedNotebook()
    }

    func deleteNotebook(_ id: UUID) {
        guard !isRunning, let index = notebooks.firstIndex(where: { $0.id == id }) else { return }
        notebooks.remove(at: index)
        if notebooks.isEmpty {
            notebooks = [AgentNotebook(title: L10n.string("Untitled Notebook"))]
        }
        if selectedNotebookID == id {
            selectedNotebookID = notebooks[min(index, notebooks.count - 1)].id
            restoreSelectedNotebook()
        }
        persistNotebooks()
    }

    func renameSelectedNotebook(_ title: String) {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        updateSelectedNotebook { $0.title = trimmed }
    }

    func isSourceSelected(_ path: String) -> Bool {
        selectedSourcePaths.contains(path)
    }

    func toggleSource(_ path: String) {
        guard !isRunning else { return }
        updateSelectedNotebook { notebook in
            if let index = notebook.sourcePaths.firstIndex(of: path) {
                notebook.sourcePaths.remove(at: index)
            } else {
                notebook.sourcePaths.append(path)
                notebook.sourcePaths.sort {
                    $0.localizedStandardCompare($1) == .orderedAscending
                }
            }
        }
    }

    func importSourceFiles(_ urls: [URL]) async {
        guard !urls.isEmpty, !isRunning, !isImportingSources, selectedNotebook != nil else {
            return
        }
        isImportingSources = true
        errorMessage = nil
        defer { isImportingSources = false }

        var imported: [FileEntryDto] = []
        var failures: [String] = []
        for url in urls {
            let isAccessing = url.startAccessingSecurityScopedResource()
            defer {
                if isAccessing {
                    url.stopAccessingSecurityScopedResource()
                }
            }
            do {
                let values = try url.resourceValues(forKeys: [.fileSizeKey])
                if let fileSize = values.fileSize, fileSize > 1_048_576 {
                    throw SourceImportError.fileTooLarge(url.lastPathComponent)
                }
                let data = try Data(contentsOf: url, options: .mappedIfSafe)
                let entry = try await service.importLibrarySource(
                    fileName: url.lastPathComponent,
                    contents: data
                )
                imported.append(entry)
            } catch {
                failures.append("\(url.lastPathComponent): \(error.localizedDescription)")
            }
        }

        if !imported.isEmpty {
            let importedPaths = Set(imported.map(\.relativePath))
            availableSources.removeAll { importedPaths.contains($0.relativePath) }
            availableSources.append(contentsOf: imported)
            updateSelectedNotebook { notebook in
                notebook.sourcePaths.append(contentsOf: imported.map(\.relativePath))
                notebook.sourcePaths = Array(Set(notebook.sourcePaths)).sorted {
                    $0.localizedStandardCompare($1) == .orderedAscending
                }
            }
            await onLibraryChanged?()
        }
        if !failures.isEmpty {
            errorMessage = failures.joined(separator: "\n")
        }
    }

    func restoreAIConfiguration() async {
        guard !isAuthenticating, !isConfiguringBYOK, !isAIConfigured else { return }
        let preferredProvider = UserDefaults.standard.string(forKey: providerDefaultsKey)
        if preferredProvider != AgentProvider.cloud.rawValue, await restoreStoredBYOK() {
            return
        }
        await restoreCloudSession()
        if !isAIConfigured {
            _ = await restoreStoredBYOK()
        }
    }

    func restoreCloudSession() async {
        guard cloudAccount == nil, !isAuthenticating else { return }
        let stored: StoredCloudTokens
        do {
            guard let tokens = try credentialStore.load() else { return }
            stored = tokens
        } catch {
            errorMessage = error.localizedDescription
            return
        }

        isAuthenticating = true
        defer { isAuthenticating = false }
        do {
            cloudAccount = try await service.connectCloudAI(tokens: stored)
            byokConfig = nil
            UserDefaults.standard.set(AgentProvider.cloud.rawValue, forKey: providerDefaultsKey)
            errorMessage = nil
        } catch {
            guard shouldRefresh(after: error) else {
                errorMessage = error.localizedDescription
                return
            }
            do {
                let refreshed = try await service.refreshCloudAI(
                    refreshToken: stored.refreshToken
                )
                let tokens = StoredCloudTokens(
                    accessToken: refreshed.accessToken,
                    refreshToken: refreshed.refreshToken
                )
                try credentialStore.save(tokens)
                cloudAccount = try await service.connectCloudAI(tokens: tokens)
                byokConfig = nil
                UserDefaults.standard.set(AgentProvider.cloud.rawValue, forKey: providerDefaultsKey)
                errorMessage = nil
            } catch {
                try? credentialStore.clear()
                cloudAccount = nil
                errorMessage = error.localizedDescription
            }
        }
    }

    func saveBYOK(apiKey: String, baseURL: String, model: String) {
        guard !isRunning, !isAuthenticating, !isConfiguringBYOK else { return }
        isConfiguringBYOK = true
        errorMessage = nil
        Task { [weak self] in
            guard let self else { return }
            defer { isConfiguringBYOK = false }
            let previous = try? byokCredentialStore.load()
            do {
                let trimmedBaseURL = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
                let trimmedModel = model.trimmingCharacters(in: .whitespacesAndNewlines)
                let existingKey = previous?.apiKey
                let trimmedKey = apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
                let resolvedKey = trimmedKey.isEmpty ? existingKey : trimmedKey
                guard let resolvedKey, !resolvedKey.isEmpty else {
                    throw BYOKConfigurationError.apiKeyRequired
                }
                let stored = StoredBYOKConfig(
                    apiKey: resolvedKey,
                    baseURL: trimmedBaseURL,
                    model: trimmedModel
                )
                try byokCredentialStore.save(stored)
                do {
                    byokConfig = try await service.connectBYOK(
                        apiKey: stored.apiKey,
                        baseURL: stored.baseURL,
                        model: stored.model
                    )
                } catch {
                    if let previous {
                        try? byokCredentialStore.save(previous)
                    } else {
                        try? byokCredentialStore.clear()
                    }
                    throw error
                }
                hasSavedBYOK = true
                cloudAccount = nil
                UserDefaults.standard.set(AgentProvider.byok.rawValue, forKey: providerDefaultsKey)
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func useSavedBYOK() {
        guard !isRunning, !isAuthenticating, !isConfiguringBYOK else { return }
        do {
            guard let stored = try byokCredentialStore.load() else {
                errorMessage = L10n.string("Save a BYOK API key before using it.")
                return
            }
            saveBYOK(apiKey: stored.apiKey, baseURL: stored.baseURL, model: stored.model)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func removeBYOK() {
        guard !isRunning, !isAuthenticating, !isConfiguringBYOK else { return }
        isConfiguringBYOK = true
        errorMessage = nil
        Task { [weak self] in
            guard let self else { return }
            defer { isConfiguringBYOK = false }
            do {
                try await service.disconnectBYOK()
                try byokCredentialStore.clear()
                byokConfig = nil
                hasSavedBYOK = false
                if UserDefaults.standard.string(forKey: providerDefaultsKey) == AgentProvider.byok.rawValue {
                    UserDefaults.standard.removeObject(forKey: providerDefaultsKey)
                }
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    private func restoreStoredBYOK() async -> Bool {
        let stored: StoredBYOKConfig
        do {
            guard let value = try byokCredentialStore.load() else { return false }
            stored = value
            hasSavedBYOK = true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }

        isConfiguringBYOK = true
        defer { isConfiguringBYOK = false }
        do {
            byokConfig = try await service.connectBYOK(
                apiKey: stored.apiKey,
                baseURL: stored.baseURL,
                model: stored.model
            )
            cloudAccount = nil
            UserDefaults.standard.set(AgentProvider.byok.rawValue, forKey: providerDefaultsKey)
            errorMessage = nil
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    func signIn() {
        guard !isAuthenticating, !isRunning else { return }
        isAuthenticating = true
        errorMessage = nil
        Task {
            defer { isAuthenticating = false }
            do {
                let loginURL = try await service.cloudAILoginURL()
                let callbackURL = try await authPresenter.authenticate(loginURL: loginURL)
                let result = try await service.parseCloudAIAuthCallback(callbackURL)
                let tokens = StoredCloudTokens(
                    accessToken: result.accessToken,
                    refreshToken: result.refreshToken
                )
                try credentialStore.save(tokens)
                cloudAccount = try await service.connectCloudAI(tokens: tokens)
                byokConfig = nil
                UserDefaults.standard.set(AgentProvider.cloud.rawValue, forKey: providerDefaultsKey)
            } catch CloudPlatformAuthError.cancelled {
                return
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    func signOut() {
        guard !isRunning, !isAuthenticating else { return }
        isAuthenticating = true
        errorMessage = nil
        Task {
            defer {
                cloudAccount = nil
                isAuthenticating = false
            }
            do {
                try await service.disconnectCloudAI()
            } catch {
                errorMessage = L10n.format(
                    "Signed out locally. Cloud AI could not revoke the remote session: %@",
                    error.localizedDescription
                )
            }
            do {
                try credentialStore.clear()
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }

    private func consume(_ event: AgentEventDto) async {
        events.append(event)
        if let eventStatus = event.status {
            status = eventStatus
        }
        switch event.kind {
        case .textDelta:
            let delta = event.text ?? ""
            guard !delta.isEmpty else { break }
            answer += delta
            let messageID = activeAssistantMessageID ?? UUID()
            if activeAssistantMessageID == nil {
                activeAssistantMessageID = messageID
            }
            updateSelectedNotebook(persist: false) { notebook in
                if let index = notebook.messages.firstIndex(where: { $0.id == messageID }) {
                    notebook.messages[index].content += delta
                } else {
                    notebook.messages.append(AgentChatMessage(
                        id: messageID,
                        role: .assistant,
                        content: delta
                    ))
                }
                notebook.lastAnswer = answer
            }
        case .confirmationRequired:
            pendingConfirmation = PendingConfirmation(
                id: event.confirmationId ?? UUID().uuidString,
                runID: event.runId,
                toolName: event.toolName ?? L10n.string("Tool"),
                preview: event.preview ?? L10n.string("Write to Workspace"),
                permission: event.permission ?? .write,
                payloadJSON: event.payloadJson ?? "{}"
            )
        case .inputRequired:
            let payload = event.payloadJson
                .flatMap { $0.data(using: .utf8) }
                .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
            pendingInput = PendingAgentInput(
                id: event.confirmationId ?? UUID().uuidString,
                runID: event.runId,
                prompt: payload?["prompt"] as? String
                    ?? event.preview
                    ?? L10n.string("What should the Agent know?"),
                options: payload?["options"] as? [String] ?? []
            )
        case .toolCompleted:
            if event.toolName == "create_task" {
                await onTasksChanged?()
            }
        case .failed:
            errorMessage = event.text ?? L10n.string("Agent failed.")
            isRunning = false
            updateSelectedNotebook { $0.lastAnswer = answer }
            activeAssistantMessageID = nil
            if let message = event.text, shouldRefresh(afterMessage: message) {
                cloudAccount = nil
                await restoreAIConfiguration()
            }
        case .cancelled, .completed:
            isRunning = false
            updateSelectedNotebook { $0.lastAnswer = answer }
            activeAssistantMessageID = nil
        default:
            break
        }
    }

    private func shouldRefresh(after error: any Error) -> Bool {
        shouldRefresh(afterMessage: error.localizedDescription)
    }

    private func shouldRefresh(afterMessage message: String) -> Bool {
        let message = message.lowercased()
        return message.contains("session expired")
            || message.contains("invalid or expired session")
            || message.contains("session_expired")
    }

    private func restoreSelectedNotebook() {
        goal = ""
        answer = selectedNotebook?.messages.last(where: { $0.role == .assistant })?.content ?? ""
        activeAssistantMessageID = nil
        events = []
        status = nil
        mode = .chat
        errorMessage = nil
    }

    private func updateSelectedNotebook(
        persist: Bool = true,
        _ update: (inout AgentNotebook) -> Void
    ) {
        guard
            let selectedNotebookID,
            let index = notebooks.firstIndex(where: { $0.id == selectedNotebookID })
        else {
            return
        }
        update(&notebooks[index])
        notebooks[index].updatedAt = .now
        if persist {
            persistNotebooks()
        }
    }

    private func persistNotebooks() {
        guard let workspaceID else { return }
        let snapshot = notebooks.map(\.dto)
        Task {
            do {
                try await service.saveAgentNotebooks(
                    workspaceID: workspaceID,
                    notebooks: snapshot
                )
            } catch {
                guard self.workspaceID == workspaceID else { return }
                errorMessage = error.localizedDescription
            }
        }
    }
}

private enum SourceImportError: LocalizedError {
    case fileTooLarge(String)

    var errorDescription: String? {
        switch self {
        case let .fileTooLarge(name):
            L10n.format("%@ exceeds the 1 MiB text source limit.", name)
        }
    }
}

private enum BYOKConfigurationError: LocalizedError {
    case apiKeyRequired

    var errorDescription: String? {
        switch self {
        case .apiKeyRequired:
            L10n.string("Enter an API key before saving BYOK.")
        }
    }
}
