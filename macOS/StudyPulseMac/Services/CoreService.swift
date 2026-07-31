import Foundation

protocol CoreServicing: Sendable {
    func createWorkspace(path: String) async throws -> WorkspaceDto
    func openWorkspace(path: String) async throws -> WorkspaceDto
    func closeWorkspace() async throws

    func cloudAILoginURL() async throws -> String
    func parseCloudAIAuthCallback(_ callbackURL: String) async throws -> CloudAuthTokensDto
    func connectCloudAI(tokens: StoredCloudTokens) async throws -> CloudAccountDto
    func refreshCloudAI(refreshToken: String) async throws -> CloudAuthTokensDto
    func disconnectCloudAI() async throws
    func connectBYOK(apiKey: String, baseURL: String, model: String) async throws -> ByokConfigDto
    func disconnectBYOK() async throws

    func startAgent(
        mode: AgentModeDto,
        goal: String,
        sourcePaths: [String],
        history: [AgentMessageDto]
    ) async throws -> String
    func listAgentCapabilities() async throws -> [CapabilityManifestDto]
    func cancelAgent(runID: String) async throws
    func submitConfirmation(
        runID: String,
        confirmationID: String,
        decision: ConfirmationDecisionDto
    ) async throws
    func submitAgentInput(
        runID: String,
        inputID: String,
        answerJSON: String
    ) async throws
    func agentEvents(runID: String) -> AsyncThrowingStream<AgentEventDto, any Error>

    func getTasks() async throws -> [TaskDto]
    func upsertTask(_ task: TaskDto) async throws
    func deleteTask(id: String) async throws
    func setTaskCompleted(id: String, completed: Bool) async throws
    func getSubjects() async throws -> [SubjectDto]
    func upsertSubject(_ value: SubjectDto) async throws
    func deleteSubject(id: String) async throws
    func getPhases() async throws -> [StudyPhaseDto]
    func upsertPhase(_ value: StudyPhaseDto) async throws
    func deletePhase(id: String) async throws
    func getGrades() async throws -> [GradeDto]
    func upsertGrade(_ value: GradeDto) async throws
    func deleteGrade(id: String) async throws
    func getMistakes() async throws -> [MistakeNoteDto]
    func getDueMistakes() async throws -> [MistakeNoteDto]
    func upsertMistake(_ value: MistakeNoteDto) async throws
    func deleteMistake(id: String) async throws
    func reviewMistake(id: String, quality: Int64) async throws -> SrsReviewResultDto
    func getExams() async throws -> [ExamDto]
    func upsertExam(_ value: ExamDto) async throws
    func deleteExam(id: String) async throws
    func getComprehensiveExams() async throws -> [ComprehensiveExamDto]
    func upsertComprehensiveExam(_ value: ComprehensiveExamDto) async throws
    func deleteComprehensiveExam(id: String) async throws
    func getRoutines() async throws -> [RoutineDto]
    func getRoutineInstances() async throws -> [RoutineInstanceDto]
    func upsertRoutine(_ value: RoutineDto) async throws
    func upsertRoutineInstance(_ value: RoutineInstanceDto) async throws
    func deleteRoutine(id: String) async throws
    func deleteRoutineInstance(id: String) async throws
    func getStudySessions() async throws -> [StudySessionDto]
    func upsertStudySession(_ value: StudySessionDto) async throws
    func deleteStudySession(id: String) async throws
    func getTimeInvestmentSubjects() async throws -> [TimeInvestmentSubjectDto]
    func upsertTimeInvestmentSubject(_ value: TimeInvestmentSubjectDto) async throws
    func deleteTimeInvestmentSubject(id: String) async throws
    func getSubTasks() async throws -> [SubTaskDto]
    func upsertSubTask(_ value: SubTaskDto) async throws
    func deleteSubTask(id: String) async throws
    func getGoalRewards() async throws -> [GoalRewardDto]
    func upsertGoalReward(_ value: GoalRewardDto) async throws
    func deleteGoalReward(id: String) async throws
    func getTimeInvestmentSummary() async throws -> [TimeInvestmentSummaryDto]
    func getTodaySnapshot() async throws -> TodaySnapshotDto
    func startTimer(intensity: SessionIntensityDto, targetDurationSeconds: Int64, investmentTarget: InvestmentTargetDto?) async throws -> TimerSnapshotDto
    func pauseTimer() async throws -> TimerSnapshotDto
    func resumeTimer() async throws -> TimerSnapshotDto
    func finishTimer() async throws -> StudySessionDto
    func cancelTimer() async throws
    func activeTimer() async -> TimerSnapshotDto
    func readMedia(relativePath: String) async throws -> Data
    func writeMedia(relativePath: String, contents: Data) async throws -> String
    func exportBackup(options: BackupExportOptionsDto) async throws -> BackupExportResultDto
    func getAgentNotebooks() async throws -> [AgentNotebookDto]
    func saveAgentNotebooks(workspaceID: String, notebooks: [AgentNotebookDto]) async throws
    func importLibrarySource(fileName: String, contents: Data) async throws -> FileEntryDto
    func listLibraryFiles() async throws -> [FileEntryDto]
    func searchLibrary(query: String) async throws -> [SearchMatchDto]

    func inspectBackup(path: String) async throws -> BackupInspectionDto
    func applyBackup(
        inspectionID: String,
        mode: RestoreModeDto,
        resolutions: [BackupResolutionDto]
    ) async throws -> ImportReportDto
    func cancelBackup(inspectionID: String) async throws
}

final class LiveCoreService: CoreServicing, @unchecked Sendable {
    private let core = StudyPulseCore()
    private let queue = DispatchQueue(label: "space.chenkai.StudyPulse.core", qos: .userInitiated)

    func createWorkspace(path: String) async throws -> WorkspaceDto {
        try await perform { try $0.createWorkspace(path: path) }
    }

    func openWorkspace(path: String) async throws -> WorkspaceDto {
        try await perform { try $0.openWorkspace(path: path) }
    }

    func closeWorkspace() async throws {
        try await perform { try $0.closeWorkspace() }
    }

    func cloudAILoginURL() async throws -> String {
        try await perform { try $0.cloudAiLoginUrl() }
    }

    func parseCloudAIAuthCallback(_ callbackURL: String) async throws -> CloudAuthTokensDto {
        try await perform { try $0.parseCloudAiAuthCallback(callbackUrl: callbackURL) }
    }

    func connectCloudAI(tokens: StoredCloudTokens) async throws -> CloudAccountDto {
        try await perform {
            try $0.connectCloudAi(
                accessToken: tokens.accessToken,
                refreshToken: tokens.refreshToken
            )
        }
    }

    func refreshCloudAI(refreshToken: String) async throws -> CloudAuthTokensDto {
        try await perform { try $0.refreshCloudAi(refreshToken: refreshToken) }
    }

    func disconnectCloudAI() async throws {
        try await perform { try $0.disconnectCloudAi() }
    }

    func connectBYOK(apiKey: String, baseURL: String, model: String) async throws -> ByokConfigDto {
        try await perform {
            try $0.connectByok(apiKey: apiKey, baseUrl: baseURL, model: model)
        }
    }

    func disconnectBYOK() async throws {
        try await perform { try $0.disconnectByok() }
    }

    func startAgent(
        mode: AgentModeDto,
        goal: String,
        sourcePaths: [String],
        history: [AgentMessageDto]
    ) async throws -> String {
        try await perform {
            try $0.startAgentWithMode(mode: mode, goal: goal, sourcePaths: sourcePaths, history: history)
        }
    }

    func listAgentCapabilities() async throws -> [CapabilityManifestDto] {
        try await perform { $0.listAgentCapabilities() }
    }

    func cancelAgent(runID: String) async throws {
        try await perform { try $0.cancelAgent(runId: runID) }
    }

    func submitConfirmation(
        runID: String,
        confirmationID: String,
        decision: ConfirmationDecisionDto
    ) async throws {
        try await perform {
            try $0.submitConfirmation(
                runId: runID,
                confirmationId: confirmationID,
                decision: decision
            )
        }
    }

    func submitAgentInput(
        runID: String,
        inputID: String,
        answerJSON: String
    ) async throws {
        try await perform {
            try $0.submitAgentInput(runId: runID, inputId: inputID, answerJson: answerJSON)
        }
    }

    func agentEvents(runID: String) -> AsyncThrowingStream<AgentEventDto, any Error> {
        AsyncThrowingStream { continuation in
            let task = Task.detached { [self] in
                var cursor: UInt64 = 0
                do {
                    while !Task.isCancelled {
                        let currentCursor = cursor
                        let events = try await perform {
                            try $0.waitForAgentEvents(
                                runId: runID,
                                afterSequence: currentCursor,
                                timeoutMs: 1_000
                            )
                        }
                        for event in events {
                            cursor = max(cursor, event.sequence)
                            continuation.yield(event)
                        }
                        if events.contains(where: { $0.kind.isTerminal }) {
                            continuation.finish()
                            return
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    func getTasks() async throws -> [TaskDto] {
        try await perform { try $0.getTasks() }
    }

    func upsertTask(_ task: TaskDto) async throws {
        try await perform { try $0.upsertTask(task: task) }
    }

    func deleteTask(id: String) async throws {
        try await perform { try $0.deleteTask(id: id) }
    }

    func setTaskCompleted(id: String, completed: Bool) async throws {
        try await perform { try $0.setTaskCompleted(id: id, completed: completed) }
    }

    func getSubjects() async throws -> [SubjectDto] {
        try await perform { try $0.getSubjects() }
    }

    func upsertSubject(_ value: SubjectDto) async throws {
        try await perform { try $0.upsertSubject(value: value) }
    }

    func deleteSubject(id: String) async throws {
        try await perform { try $0.deleteSubject(id: id) }
    }

    func getPhases() async throws -> [StudyPhaseDto] {
        try await perform { try $0.getPhases() }
    }

    func upsertPhase(_ value: StudyPhaseDto) async throws {
        try await perform { try $0.upsertPhase(value: value) }
    }

    func deletePhase(id: String) async throws {
        try await perform { try $0.deletePhase(id: id) }
    }

    func getGrades() async throws -> [GradeDto] {
        try await perform { try $0.getGrades() }
    }

    func upsertGrade(_ value: GradeDto) async throws {
        try await perform { try $0.upsertGrade(value: value) }
    }

    func deleteGrade(id: String) async throws {
        try await perform { try $0.deleteGrade(id: id) }
    }

    func getMistakes() async throws -> [MistakeNoteDto] {
        try await perform { try $0.getMistakes() }
    }

    func getDueMistakes() async throws -> [MistakeNoteDto] {
        try await perform { try $0.getDueMistakes() }
    }

    func upsertMistake(_ value: MistakeNoteDto) async throws {
        try await perform { try $0.upsertMistake(value: value) }
    }

    func deleteMistake(id: String) async throws {
        try await perform { try $0.deleteMistake(id: id) }
    }

    func reviewMistake(id: String, quality: Int64) async throws -> SrsReviewResultDto {
        try await perform { try $0.reviewMistake(id: id, quality: quality) }
    }

    func getExams() async throws -> [ExamDto] {
        try await perform { try $0.getExams() }
    }

    func upsertExam(_ value: ExamDto) async throws {
        try await perform { try $0.upsertExam(value: value) }
    }

    func deleteExam(id: String) async throws {
        try await perform { try $0.deleteExam(id: id) }
    }

    func getComprehensiveExams() async throws -> [ComprehensiveExamDto] {
        try await perform { try $0.getComprehensiveExams() }
    }

    func upsertComprehensiveExam(_ value: ComprehensiveExamDto) async throws {
        try await perform { try $0.upsertComprehensiveExam(value: value) }
    }

    func deleteComprehensiveExam(id: String) async throws {
        try await perform { try $0.deleteComprehensiveExam(id: id) }
    }

    func getRoutines() async throws -> [RoutineDto] {
        try await perform { try $0.getRoutines() }
    }

    func getRoutineInstances() async throws -> [RoutineInstanceDto] {
        try await perform { try $0.getRoutineInstances() }
    }

    func upsertRoutine(_ value: RoutineDto) async throws {
        try await perform { try $0.upsertRoutine(value: value) }
    }

    func upsertRoutineInstance(_ value: RoutineInstanceDto) async throws {
        try await perform { try $0.upsertRoutineInstance(value: value) }
    }

    func deleteRoutine(id: String) async throws {
        try await perform { try $0.deleteRoutine(id: id) }
    }

    func deleteRoutineInstance(id: String) async throws {
        try await perform { try $0.deleteRoutineInstance(id: id) }
    }

    func getStudySessions() async throws -> [StudySessionDto] {
        try await perform { try $0.getStudySessions() }
    }

    func upsertStudySession(_ value: StudySessionDto) async throws {
        try await perform { try $0.upsertStudySession(value: value) }
    }

    func deleteStudySession(id: String) async throws {
        try await perform { try $0.deleteStudySession(id: id) }
    }

    func getTimeInvestmentSubjects() async throws -> [TimeInvestmentSubjectDto] {
        try await perform { try $0.getTimeInvestmentSubjects() }
    }

    func upsertTimeInvestmentSubject(_ value: TimeInvestmentSubjectDto) async throws {
        try await perform { try $0.upsertTimeInvestmentSubject(value: value) }
    }

    func deleteTimeInvestmentSubject(id: String) async throws {
        try await perform { try $0.deleteTimeInvestmentSubject(id: id) }
    }

    func getSubTasks() async throws -> [SubTaskDto] {
        try await perform { try $0.getSubTasks() }
    }

    func upsertSubTask(_ value: SubTaskDto) async throws {
        try await perform { try $0.upsertSubTask(value: value) }
    }

    func deleteSubTask(id: String) async throws {
        try await perform { try $0.deleteSubTask(id: id) }
    }

    func getGoalRewards() async throws -> [GoalRewardDto] {
        try await perform { try $0.getGoalRewards() }
    }

    func upsertGoalReward(_ value: GoalRewardDto) async throws {
        try await perform { try $0.upsertGoalReward(value: value) }
    }

    func deleteGoalReward(id: String) async throws {
        try await perform { try $0.deleteGoalReward(id: id) }
    }

    func getTimeInvestmentSummary() async throws -> [TimeInvestmentSummaryDto] {
        try await perform { try $0.getTimeInvestmentSummary() }
    }

    func getTodaySnapshot() async throws -> TodaySnapshotDto {
        try await perform { try $0.getTodaySnapshot() }
    }

    func startTimer(
        intensity: SessionIntensityDto,
        targetDurationSeconds: Int64,
        investmentTarget: InvestmentTargetDto?
    ) async throws -> TimerSnapshotDto {
        try await perform {
            try $0.startTimer(
                intensity: intensity,
                targetDurationSeconds: targetDurationSeconds,
                investmentTarget: investmentTarget
            )
        }
    }

    func pauseTimer() async throws -> TimerSnapshotDto {
        try await perform { try $0.pauseTimer() }
    }

    func resumeTimer() async throws -> TimerSnapshotDto {
        try await perform { try $0.resumeTimer() }
    }

    func finishTimer() async throws -> StudySessionDto {
        try await perform { try $0.finishTimer() }
    }

    func cancelTimer() async throws {
        try await perform { try $0.cancelTimer() }
    }

    func activeTimer() async -> TimerSnapshotDto {
        try! await perform { $0.activeTimer() }
    }

    func readMedia(relativePath: String) async throws -> Data {
        try await perform { try $0.readMedia(relativePath: relativePath) }
    }

    func writeMedia(relativePath: String, contents: Data) async throws -> String {
        try await perform { try $0.writeMedia(relativePath: relativePath, contents: contents) }
    }

    func exportBackup(options: BackupExportOptionsDto) async throws -> BackupExportResultDto {
        try await perform { try $0.exportBackup(options: options) }
    }

    func getAgentNotebooks() async throws -> [AgentNotebookDto] {
        try await perform { try $0.getAgentNotebooks() }
    }

    func saveAgentNotebooks(
        workspaceID: String,
        notebooks: [AgentNotebookDto]
    ) async throws {
        try await perform {
            try $0.saveAgentNotebooks(workspaceId: workspaceID, notebooks: notebooks)
        }
    }

    func importLibrarySource(fileName: String, contents: Data) async throws -> FileEntryDto {
        try await perform {
            try $0.importLibrarySource(
                fileName: fileName,
                contents: contents
            )
        }
    }

    func listLibraryFiles() async throws -> [FileEntryDto] {
        try await perform { try $0.listLibraryFiles() }
    }

    func searchLibrary(query: String) async throws -> [SearchMatchDto] {
        try await perform { try $0.searchLibrary(query: query) }
    }

    func inspectBackup(path: String) async throws -> BackupInspectionDto {
        try await perform { try $0.inspectBackup(archivePath: path) }
    }

    func applyBackup(
        inspectionID: String,
        mode: RestoreModeDto,
        resolutions: [BackupResolutionDto]
    ) async throws -> ImportReportDto {
        try await perform {
            try $0.applyBackup(
                inspectionId: inspectionID,
                mode: mode,
                resolutions: resolutions
            )
        }
    }

    func cancelBackup(inspectionID: String) async throws {
        try await perform { try $0.cancelBackup(inspectionId: inspectionID) }
    }

    private func perform<T: Sendable>(
        _ body: @escaping @Sendable (StudyPulseCore) throws -> T
    ) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { [core] in
                continuation.resume(with: Result { try body(core) })
            }
        }
    }
}

private extension AgentEventKindDto {
    nonisolated var isTerminal: Bool {
        switch self {
        case .failed, .cancelled, .completed: true
        default: false
        }
    }
}
