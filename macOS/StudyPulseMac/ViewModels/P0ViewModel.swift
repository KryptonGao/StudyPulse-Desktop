import Combine
import Foundation

@MainActor
final class P0ViewModel: ObservableObject {
    @Published private(set) var subjects: [SubjectDto] = []
    @Published private(set) var phases: [StudyPhaseDto] = []
    @Published private(set) var grades: [GradeDto] = []
    @Published private(set) var mistakes: [MistakeNoteDto] = []
    @Published private(set) var dueMistakes: [MistakeNoteDto] = []
    @Published private(set) var exams: [ExamDto] = []
    @Published private(set) var sessions: [StudySessionDto] = []
    @Published private(set) var investmentSubjects: [TimeInvestmentSubjectDto] = []
    @Published private(set) var investmentSummary: [TimeInvestmentSummaryDto] = []
    @Published private(set) var today: TodaySnapshotDto?
    @Published private(set) var timer: TimerSnapshotDto
    @Published var selectedSubjectID: String?
    @Published var selectedExamID: String?
    @Published var selectedMistakeID: String?
    @Published var selectedInvestmentSubjectID: String?
    @Published private(set) var isLoading = false
    @Published var errorMessage: String?

    private let service: any CoreServicing

    init(service: any CoreServicing) {
        self.service = service
        timer = TimerSnapshotDto(
            status: .idle,
            sessionId: nil,
            startedAt: nil,
            elapsedSeconds: 0,
            targetDurationSeconds: 0,
            intensity: nil,
            investmentTarget: nil
        )
    }

    var selectedSubject: SubjectDto? {
        guard let selectedSubjectID else { return nil }
        return subjects.first { $0.id == selectedSubjectID }
    }

    var selectedExam: ExamDto? {
        guard let selectedExamID else { return nil }
        return exams.first { $0.id == selectedExamID }
    }

    var selectedMistake: MistakeNoteDto? {
        guard let selectedMistakeID else { return nil }
        return mistakes.first { $0.id == selectedMistakeID }
    }

    var selectedInvestmentSubject: TimeInvestmentSubjectDto? {
        guard let selectedInvestmentSubjectID else { return nil }
        return investmentSubjects.first { $0.id == selectedInvestmentSubjectID }
    }

    func refresh() async {
        guard !isLoading else { return }
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }
        do {
            async let subjects = service.getSubjects()
            async let phases = service.getPhases()
            async let grades = service.getGrades()
            async let mistakes = service.getMistakes()
            async let dueMistakes = service.getDueMistakes()
            async let exams = service.getExams()
            async let sessions = service.getStudySessions()
            async let investmentSubjects = service.getTimeInvestmentSubjects()
            async let investmentSummary = service.getTimeInvestmentSummary()
            async let today = service.getTodaySnapshot()
            self.subjects = try await subjects
            self.phases = try await phases
            self.grades = try await grades
            self.mistakes = try await mistakes
            self.dueMistakes = try await dueMistakes
            self.exams = try await exams
            self.sessions = try await sessions
            self.investmentSubjects = try await investmentSubjects
            self.investmentSummary = try await investmentSummary
            self.today = try await today
            timer = await service.activeTimer()
            normalizeSelections()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func reset() {
        subjects = []
        phases = []
        grades = []
        mistakes = []
        dueMistakes = []
        exams = []
        sessions = []
        investmentSubjects = []
        investmentSummary = []
        today = nil
        selectedSubjectID = nil
        selectedExamID = nil
        selectedMistakeID = nil
        selectedInvestmentSubjectID = nil
        timer = TimerSnapshotDto(
            status: .idle,
            sessionId: nil,
            startedAt: nil,
            elapsedSeconds: 0,
            targetDurationSeconds: 0,
            intensity: nil,
            investmentTarget: nil
        )
        errorMessage = nil
    }

    func addSubject(name: String, displayName: String) async {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            let value = SubjectDto(
                id: UUID().uuidString.lowercased(),
                name: trimmed,
                enabled: true,
                fullScore: 100,
                displayName: displayName.isEmpty ? trimmed : displayName,
                extraJson: "{}"
            )
            try await service.upsertSubject(value)
            subjects = try await service.getSubjects()
            selectedSubjectID = value.id
        } catch { errorMessage = error.localizedDescription }
    }

    func deleteSubject(_ id: String) async {
        do {
            try await service.deleteSubject(id: id)
            subjects = try await service.getSubjects()
            if selectedSubjectID == id { selectedSubjectID = nil }
        } catch { errorMessage = error.localizedDescription }
    }

    func addExam(name: String, subject: String) async {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            let now = Date().iso8601String
            let value = ExamDto(
                id: UUID().uuidString.lowercased(),
                name: trimmed,
                examDate: now,
                examEndDate: nil,
                importance: 3,
                subject: subject,
                examName: trimmed,
                masteryDegree: 0,
                timeSlot: nil,
                phaseId: nil,
                checklist: [],
                locationSchool: "",
                locationClassroom: "",
                locationSeat: "",
                countdownNotifyDays: nil,
                examReview: nil,
                extraJson: "{}"
            )
            try await service.upsertExam(value)
            exams = try await service.getExams()
            selectedExamID = value.id
        } catch { errorMessage = error.localizedDescription }
    }

    func deleteExam(_ id: String) async {
        do {
            try await service.deleteExam(id: id)
            exams = try await service.getExams()
            if selectedExamID == id { selectedExamID = nil }
        } catch { errorMessage = error.localizedDescription }
    }

    func addInvestmentSubject(name: String) async {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        do {
            let now = ISO8601DateFormatter().string(from: .now)
            let value = TimeInvestmentSubjectDto(
                id: UUID().uuidString.lowercased(),
                name: trimmed,
                symbolName: "chart.bar.fill",
                theme: .ocean,
                startDate: now,
                sortOrder: Int64(investmentSubjects.count),
                createdAt: now,
                isArchived: false,
                extraJson: "{}"
            )
            try await service.upsertTimeInvestmentSubject(value)
            investmentSubjects = try await service.getTimeInvestmentSubjects()
            investmentSummary = try await service.getTimeInvestmentSummary()
            selectedInvestmentSubjectID = value.id
        } catch { errorMessage = error.localizedDescription }
    }

    func deleteMistake(_ id: String) async {
        do {
            try await service.deleteMistake(id: id)
            mistakes = try await service.getMistakes()
            dueMistakes = try await service.getDueMistakes()
            if selectedMistakeID == id { selectedMistakeID = nil }
        } catch { errorMessage = error.localizedDescription }
    }

    func reviewMistake(_ id: String, quality: Int64) async {
        do {
            _ = try await service.reviewMistake(id: id, quality: quality)
            mistakes = try await service.getMistakes()
            dueMistakes = try await service.getDueMistakes()
            today = try await service.getTodaySnapshot()
            normalizeSelections()
        } catch { errorMessage = error.localizedDescription }
    }

    func startTimer(intensity: SessionIntensityDto) async {
        do {
            timer = try await service.startTimer(
                intensity: intensity,
                targetDurationSeconds: recommendedDuration(for: intensity),
                investmentTarget: nil
            )
        } catch { errorMessage = error.localizedDescription }
    }

    func pauseTimer() async {
        do { timer = try await service.pauseTimer() }
        catch { errorMessage = error.localizedDescription }
    }

    func resumeTimer() async {
        do { timer = try await service.resumeTimer() }
        catch { errorMessage = error.localizedDescription }
    }

    func finishTimer() async {
        do {
            _ = try await service.finishTimer()
            timer = await service.activeTimer()
            sessions = try await service.getStudySessions()
            today = try await service.getTodaySnapshot()
        } catch { errorMessage = error.localizedDescription }
    }

    func cancelTimer() async {
        do { try await service.cancelTimer(); timer = await service.activeTimer() }
        catch { errorMessage = error.localizedDescription }
    }

    func pollTimer() async {
        timer = await service.activeTimer()
    }

    func exportBackup(to url: URL) async {
        do {
            _ = try await service.exportBackup(
                options: BackupExportOptionsDto(
                    archivePath: url.path,
                    includesMedia: true,
                    includesDerivedHealthData: false,
                    appVersion: "0.1.0",
                    appBuild: "desktop",
                    locale: Locale.current.identifier
                )
            )
        } catch { errorMessage = error.localizedDescription }
    }

    private func recommendedDuration(for intensity: SessionIntensityDto) -> Int64 {
        switch intensity {
        case .peak: 50 * 60
        case .deepFocus: 45 * 60
        case .steady: 35 * 60
        case .light: 25 * 60
        case .recovery: 20 * 60
        }
    }

    private func normalizeSelections() {
        if let selectedSubjectID, !subjects.contains(where: { $0.id == selectedSubjectID }) {
            self.selectedSubjectID = nil
        }
        if let selectedExamID, !exams.contains(where: { $0.id == selectedExamID }) {
            self.selectedExamID = nil
        }
        if let selectedMistakeID, !mistakes.contains(where: { $0.id == selectedMistakeID }) {
            self.selectedMistakeID = nil
        }
        if let selectedInvestmentSubjectID,
           !investmentSubjects.contains(where: { $0.id == selectedInvestmentSubjectID }) {
            self.selectedInvestmentSubjectID = nil
        }
    }
}

private extension Date {
    var iso8601String: String {
        ISO8601DateFormatter().string(from: self)
    }
}
