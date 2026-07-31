import Foundation
import Testing
@testable import StudyPulseMac

@MainActor
struct StudyPulseDesktopTests {
    @Test
    func taskGroupingUsesPortableIsoDates() async throws {
        let now = Date()
        let service = TestCoreService(
            tasks: [
                makeTask(
                    title: "This week",
                    dueDate: now.addingTimeInterval(2 * 86_400)
                ),
                makeTask(
                    title: "Later",
                    dueDate: now.addingTimeInterval(45 * 86_400)
                ),
            ]
        )
        let viewModel = TasksViewModel(service: service)

        await viewModel.refresh()

        #expect(viewModel.sections.map(\.id) == ["week", "later"])
        #expect(viewModel.sections.flatMap(\.tasks).count == 2)
    }

    @Test
    func agentMergesTextDeltasAndTerminalState() async throws {
        let source = FileEntryDto(
            relativePath: "Documents/algebra.md",
            isDirectory: false,
            sizeBytes: 128,
            modifiedAt: nil
        )
        let service = TestCoreService(events: [
            agentEvent(sequence: 1, kind: .started),
            agentEvent(sequence: 2, kind: .textDelta, text: "Study "),
            agentEvent(sequence: 3, kind: .textDelta, text: "ready"),
            agentEvent(sequence: 4, kind: .completed, status: .completed),
        ])
        let viewModel = AgentViewModel(
            service: service,
            credentialStore: TestCloudCredentialStore(tokens: StoredCloudTokens(
                accessToken: "sp_sess_test",
                refreshToken: "sp_refresh_test"
            ))
        )
        await viewModel.restoreCloudSession()
        await viewModel.configure(workspaceID: "workspace-test", files: [source])
        viewModel.toggleSource(source.relativePath)
        viewModel.goal = "Prepare"

        viewModel.start()
        for _ in 0 ..< 100 where viewModel.isRunning {
            try await Task.sleep(for: .milliseconds(10))
        }

        #expect(viewModel.answer == "Study ready")
        #expect(viewModel.events.map(\.sequence) == [1, 2, 3, 4])
        #expect(viewModel.status == .completed)
        #expect(service.startedSourcePaths == [source.relativePath])
        #expect(viewModel.goal.isEmpty)
        #expect(viewModel.selectedMessages.map(\.content) == ["Prepare", "Study ready"])
    }

    @Test
    func agentKeepsPriorMessagesAndSendsThemAsContext() async throws {
        let service = TestCoreService(events: [
            agentEvent(sequence: 1, kind: .started),
            agentEvent(sequence: 2, kind: .textDelta, text: "Agent reply"),
            agentEvent(sequence: 3, kind: .completed, status: .completed),
        ])
        let viewModel = AgentViewModel(
            service: service,
            credentialStore: TestCloudCredentialStore(tokens: StoredCloudTokens(
                accessToken: "sp_sess_test",
                refreshToken: "sp_refresh_test"
            ))
        )
        await viewModel.restoreCloudSession()
        await viewModel.configure(workspaceID: "workspace-chat", files: [])

        viewModel.goal = "First question"
        viewModel.start()
        for _ in 0 ..< 100 where viewModel.isRunning {
            try await Task.sleep(for: .milliseconds(10))
        }

        viewModel.goal = "Follow-up question"
        viewModel.start()
        for _ in 0 ..< 100 where viewModel.isRunning {
            try await Task.sleep(for: .milliseconds(10))
        }

        #expect(viewModel.selectedMessages.map(\.content) == [
            "First question",
            "Agent reply",
            "Follow-up question",
            "Agent reply",
        ])
        #expect(service.startedHistory.map(\.content) == [
            "First question",
            "Agent reply",
        ])
        #expect(viewModel.goal.isEmpty)
    }

    @Test
    func agentCanConfigureBYOKWithoutCloudAccount() async throws {
        let byokStore = TestBYOKCredentialStore()
        let viewModel = AgentViewModel(
            service: TestCoreService(),
            byokCredentialStore: byokStore
        )

        viewModel.saveBYOK(
            apiKey: "sk-test-key",
            baseURL: "https://api.example.com/v1",
            model: "example-model"
        )
        for _ in 0 ..< 100 where viewModel.isConfiguringBYOK {
            try await Task.sleep(for: .milliseconds(10))
        }

        #expect(viewModel.isBYOKConnected)
        #expect(viewModel.isAIConfigured)
        #expect(viewModel.hasSavedBYOK)
        #expect(byokStore.config?.apiKey == "sk-test-key")
        #expect(viewModel.byokConfig?.baseUrl == "https://api.example.com/v1")
        #expect(viewModel.byokConfig?.model == "example-model")
    }

    @Test
    func notebooksKeepIndependentSourceSelections() async throws {
        let algebra = FileEntryDto(
            relativePath: "Documents/algebra.md",
            isDirectory: false,
            sizeBytes: 128,
            modifiedAt: nil
        )
        let chemistry = FileEntryDto(
            relativePath: "Notes/chemistry.md",
            isDirectory: false,
            sizeBytes: 256,
            modifiedAt: nil
        )
        let viewModel = AgentViewModel(
            service: TestCoreService()
        )
        await viewModel.configure(
            workspaceID: "workspace-notebooks",
            files: [algebra, chemistry]
        )
        let firstNotebookID = try #require(viewModel.selectedNotebookID)
        viewModel.toggleSource(algebra.relativePath)

        viewModel.createNotebook()
        let secondNotebookID = try #require(viewModel.selectedNotebookID)
        viewModel.toggleSource(chemistry.relativePath)

        #expect(firstNotebookID != secondNotebookID)
        #expect(viewModel.selectedSourcePaths == [chemistry.relativePath])

        viewModel.selectNotebook(firstNotebookID)
        #expect(viewModel.selectedSourcePaths == [algebra.relativePath])
    }

    @Test
    func importedFileBecomesCurrentNotebookSource() async throws {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("StudyPulse-Source-\(UUID().uuidString).md")
        try Data("Algebra notes".utf8).write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }
        let viewModel = AgentViewModel(service: TestCoreService())
        await viewModel.configure(workspaceID: "workspace-import", files: [])

        await viewModel.importSourceFiles([url])

        let expectedPath = "Documents/\(url.lastPathComponent)"
        #expect(viewModel.selectedSourcePaths == [expectedPath])
        #expect(viewModel.selectedSources.map(\.relativePath) == [expectedPath])
        #expect(viewModel.errorMessage == nil)
    }

    @Test
    func swiftToUniFfiToRustCloudContractSmoke() async throws {
        let service = LiveCoreService()
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("StudyPulse-Smoke-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        _ = try await service.createWorkspace(path: root.path)
        let loginURL = try await service.cloudAILoginURL()
        let tokens = try await service.parseCloudAIAuthCallback(
            "studypulse://auth/callback"
                + "?access_token=sp_sess_test"
                + "&refresh_token=sp_refresh_test"
        )

        #expect(loginURL.contains("auth.chenkai.space/login"))
        #expect(tokens.accessToken == "sp_sess_test")
        #expect(tokens.refreshToken == "sp_refresh_test")
        #expect(try await service.getTasks().isEmpty)
    }

    @Test
    func webAuthenticationCompletionHandlerIsSafeOffMainActor() async throws {
        let callback = try #require(URL(
            string: "studypulse://auth/callback"
                + "?access_token=sp_sess_test"
                + "&refresh_token=sp_refresh_test"
        ))

        let result = try await withCheckedThrowingContinuation { continuation in
            let bridge = CloudWebAuthContinuation(continuation)
            let completionHandler = CloudWebAuthPresenter.completionHandler(for: bridge)
            Task.detached {
                completionHandler(callback, nil)
                completionHandler(nil, NSError(domain: "test", code: 1))
            }
        }

        #expect(result == callback.absoluteString)
    }
}

private final class TestCoreService: CoreServicing, @unchecked Sendable {
    private let tasks: [TaskDto]
    private let events: [AgentEventDto]
    private(set) var startedSourcePaths: [String] = []
    private(set) var startedHistory: [AgentMessageDto] = []

    init(tasks: [TaskDto] = [], events: [AgentEventDto] = []) {
        self.tasks = tasks
        self.events = events
    }

    func createWorkspace(path: String) async throws -> WorkspaceDto {
        WorkspaceDto(id: UUID().uuidString, rootPath: path, schemaVersion: 1)
    }

    func openWorkspace(path: String) async throws -> WorkspaceDto {
        WorkspaceDto(id: UUID().uuidString, rootPath: path, schemaVersion: 1)
    }

    func closeWorkspace() async throws {}

    func cloudAILoginURL() async throws -> String {
        "https://auth.chenkai.space/login"
    }

    func parseCloudAIAuthCallback(_ callbackURL: String) async throws -> CloudAuthTokensDto {
        CloudAuthTokensDto(
            accessToken: "sp_sess_test",
            refreshToken: "sp_refresh_test"
        )
    }

    func connectCloudAI(tokens: StoredCloudTokens) async throws -> CloudAccountDto {
        CloudAccountDto(
            email: "student@example.com",
            role: "user",
            membershipType: "free",
            membershipExpiresAt: nil,
            planName: "Free",
            availableModels: ["MiniMax-M3"]
        )
    }

    func refreshCloudAI(refreshToken: String) async throws -> CloudAuthTokensDto {
        CloudAuthTokensDto(
            accessToken: "sp_sess_refreshed",
            refreshToken: "sp_refresh_refreshed"
        )
    }

    func disconnectCloudAI() async throws {}

    func connectBYOK(apiKey: String, baseURL: String, model: String) async throws -> ByokConfigDto {
        ByokConfigDto(baseUrl: baseURL, model: model)
    }

    func disconnectBYOK() async throws {}

    func startAgent(
        mode: AgentModeDto,
        goal: String,
        sourcePaths: [String],
        history: [AgentMessageDto]
    ) async throws -> String {
        startedSourcePaths = sourcePaths
        startedHistory = history
        return "test-run"
    }
    func listAgentCapabilities() async throws -> [CapabilityManifestDto] { [] }
    func cancelAgent(runID: String) async throws {}

    func submitConfirmation(
        runID: String,
        confirmationID: String,
        decision: ConfirmationDecisionDto
    ) async throws {}

    func submitAgentInput(
        runID: String,
        inputID: String,
        answerJSON: String
    ) async throws {}

    func agentEvents(runID: String) -> AsyncThrowingStream<AgentEventDto, any Error> {
        AsyncThrowingStream { continuation in
            for event in events {
                continuation.yield(event)
            }
            continuation.finish()
        }
    }

    func getTasks() async throws -> [TaskDto] { tasks }
    func getAgentNotebooks() async throws -> [AgentNotebookDto] { [] }
    func saveAgentNotebooks(
        workspaceID: String,
        notebooks: [AgentNotebookDto]
    ) async throws {}
    func importLibrarySource(fileName: String, contents: Data) async throws -> FileEntryDto {
        FileEntryDto(
            relativePath: "Documents/\(fileName)",
            isDirectory: false,
            sizeBytes: UInt64(contents.count),
            modifiedAt: nil
        )
    }
    func listLibraryFiles() async throws -> [FileEntryDto] { [] }
    func searchLibrary(query: String) async throws -> [SearchMatchDto] { [] }

    func inspectBackup(path: String) async throws -> BackupInspectionDto {
        throw TestError.unimplemented
    }

    func applyBackup(
        inspectionID: String,
        mode: RestoreModeDto,
        resolutions: [BackupResolutionDto]
    ) async throws -> ImportReportDto {
        throw TestError.unimplemented
    }

    func cancelBackup(inspectionID: String) async throws {}
}

extension TestCoreService {
    func upsertTask(_ task: TaskDto) async throws { throw TestError.unimplemented }
    func deleteTask(id: String) async throws { throw TestError.unimplemented }
    func setTaskCompleted(id: String, completed: Bool) async throws { throw TestError.unimplemented }
    func getSubjects() async throws -> [SubjectDto] { throw TestError.unimplemented }
    func upsertSubject(_ value: SubjectDto) async throws { throw TestError.unimplemented }
    func deleteSubject(id: String) async throws { throw TestError.unimplemented }
    func getPhases() async throws -> [StudyPhaseDto] { throw TestError.unimplemented }
    func upsertPhase(_ value: StudyPhaseDto) async throws { throw TestError.unimplemented }
    func deletePhase(id: String) async throws { throw TestError.unimplemented }
    func getGrades() async throws -> [GradeDto] { throw TestError.unimplemented }
    func upsertGrade(_ value: GradeDto) async throws { throw TestError.unimplemented }
    func deleteGrade(id: String) async throws { throw TestError.unimplemented }
    func getMistakes() async throws -> [MistakeNoteDto] { throw TestError.unimplemented }
    func getDueMistakes() async throws -> [MistakeNoteDto] { throw TestError.unimplemented }
    func upsertMistake(_ value: MistakeNoteDto) async throws { throw TestError.unimplemented }
    func deleteMistake(id: String) async throws { throw TestError.unimplemented }
    func reviewMistake(id: String, quality: Int64) async throws -> SrsReviewResultDto {
        throw TestError.unimplemented
    }
    func getExams() async throws -> [ExamDto] { throw TestError.unimplemented }
    func upsertExam(_ value: ExamDto) async throws { throw TestError.unimplemented }
    func deleteExam(id: String) async throws { throw TestError.unimplemented }
    func getComprehensiveExams() async throws -> [ComprehensiveExamDto] {
        throw TestError.unimplemented
    }
    func upsertComprehensiveExam(_ value: ComprehensiveExamDto) async throws {
        throw TestError.unimplemented
    }
    func deleteComprehensiveExam(id: String) async throws { throw TestError.unimplemented }
    func getRoutines() async throws -> [RoutineDto] { throw TestError.unimplemented }
    func getRoutineInstances() async throws -> [RoutineInstanceDto] { throw TestError.unimplemented }
    func upsertRoutine(_ value: RoutineDto) async throws { throw TestError.unimplemented }
    func upsertRoutineInstance(_ value: RoutineInstanceDto) async throws {
        throw TestError.unimplemented
    }
    func deleteRoutine(id: String) async throws { throw TestError.unimplemented }
    func deleteRoutineInstance(id: String) async throws { throw TestError.unimplemented }
    func getStudySessions() async throws -> [StudySessionDto] { throw TestError.unimplemented }
    func upsertStudySession(_ value: StudySessionDto) async throws { throw TestError.unimplemented }
    func deleteStudySession(id: String) async throws { throw TestError.unimplemented }
    func getTimeInvestmentSubjects() async throws -> [TimeInvestmentSubjectDto] {
        throw TestError.unimplemented
    }
    func upsertTimeInvestmentSubject(_ value: TimeInvestmentSubjectDto) async throws {
        throw TestError.unimplemented
    }
    func deleteTimeInvestmentSubject(id: String) async throws { throw TestError.unimplemented }
    func getSubTasks() async throws -> [SubTaskDto] { throw TestError.unimplemented }
    func upsertSubTask(_ value: SubTaskDto) async throws { throw TestError.unimplemented }
    func deleteSubTask(id: String) async throws { throw TestError.unimplemented }
    func getGoalRewards() async throws -> [GoalRewardDto] { throw TestError.unimplemented }
    func upsertGoalReward(_ value: GoalRewardDto) async throws { throw TestError.unimplemented }
    func deleteGoalReward(id: String) async throws { throw TestError.unimplemented }
    func getTimeInvestmentSummary() async throws -> [TimeInvestmentSummaryDto] {
        throw TestError.unimplemented
    }
    func getTodaySnapshot() async throws -> TodaySnapshotDto { throw TestError.unimplemented }
    func startTimer(
        intensity: SessionIntensityDto,
        targetDurationSeconds: Int64,
        investmentTarget: InvestmentTargetDto?
    ) async throws -> TimerSnapshotDto {
        throw TestError.unimplemented
    }
    func pauseTimer() async throws -> TimerSnapshotDto { throw TestError.unimplemented }
    func resumeTimer() async throws -> TimerSnapshotDto { throw TestError.unimplemented }
    func finishTimer() async throws -> StudySessionDto { throw TestError.unimplemented }
    func cancelTimer() async throws { throw TestError.unimplemented }
    func activeTimer() async -> TimerSnapshotDto { fatalError("TestCoreService timer is unimplemented") }
    func readMedia(relativePath: String) async throws -> Data { throw TestError.unimplemented }
    func writeMedia(relativePath: String, contents: Data) async throws -> String {
        throw TestError.unimplemented
    }
    func exportBackup(options: BackupExportOptionsDto) async throws -> BackupExportResultDto {
        throw TestError.unimplemented
    }
}

private enum TestError: Error {
    case unimplemented
}

private struct TestCloudCredentialStore: CloudCredentialStoring {
    let tokens: StoredCloudTokens?

    func load() throws -> StoredCloudTokens? { tokens }
    func save(_ tokens: StoredCloudTokens) throws {}
    func clear() throws {}
}

private final class TestBYOKCredentialStore: BYOKCredentialStoring, @unchecked Sendable {
    private(set) var config: StoredBYOKConfig?

    func load() throws -> StoredBYOKConfig? { config }
    func save(_ config: StoredBYOKConfig) throws { self.config = config }
    func clear() throws { config = nil }
}

private func makeTask(title: String, dueDate: Date) -> TaskDto {
    let iso = ISO8601DateFormatter().string(from: dueDate)
    return TaskDto(
        id: UUID().uuidString,
        title: title,
        taskType: .homework,
        dueDate: iso,
        reminderDate: iso,
        subject: "Test",
        importance: 3,
        notes: "",
        isCompleted: false,
        reminderEventId: nil,
        reminderCalendarId: nil,
        createdAt: iso,
        phaseId: nil,
        coachExecutionData: nil,
        coachGoalId: nil,
        coachProposalId: nil,
        extraJson: ""
    )
}

private func agentEvent(
    sequence: UInt64,
    kind: AgentEventKindDto,
    text: String? = nil,
    status: RunStatusDto? = nil
) -> AgentEventDto {
    AgentEventDto(
        runId: "test-run",
        sequence: sequence,
        timestamp: "2026-07-30T09:00:00Z",
        kind: kind,
        status: status,
        text: text,
        toolCallId: nil,
        toolName: nil,
        permission: nil,
        preview: nil,
        confirmationId: nil,
        payloadJson: nil,
        mode: nil,
        stage: nil,
        progress: nil
    )
}
