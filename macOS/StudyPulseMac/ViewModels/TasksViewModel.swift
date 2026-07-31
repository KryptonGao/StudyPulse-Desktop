import Combine
import Foundation

@MainActor
final class TasksViewModel: ObservableObject {
    @Published private(set) var tasks: [TaskDto] = []
    @Published private(set) var sections: [TaskSection] = []
    @Published var selectedTaskID: String?
    @Published var filter: TaskFilter = .all {
        didSet { recompute() }
    }
    @Published var showCompleted = false {
        didSet { recompute() }
    }
    @Published private(set) var isLoading = false
    @Published var errorMessage: String?

    private let service: any CoreServicing

    init(service: any CoreServicing) {
        self.service = service
    }

    var selectedTask: TaskDto? {
        guard let selectedTaskID else { return nil }
        return tasks.first { $0.id == selectedTaskID }
    }

    func refresh() async {
        guard !isLoading else { return }
        isLoading = true
        defer { isLoading = false }
        do {
            tasks = try await service.getTasks()
            recompute()
            if let selectedTaskID, !tasks.contains(where: { $0.id == selectedTaskID }) {
                self.selectedTaskID = nil
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func reset() {
        tasks = []
        sections = []
        selectedTaskID = nil
        errorMessage = nil
    }

    func toggle(_ task: TaskDto) {
        Task {
            do {
                try await service.setTaskCompleted(id: task.id, completed: !task.isCompleted)
                await refresh()
            } catch { errorMessage = error.localizedDescription }
        }
    }

    func delete(_ task: TaskDto) {
        Task {
            do {
                try await service.deleteTask(id: task.id)
                if selectedTaskID == task.id { selectedTaskID = nil }
                await refresh()
            } catch { errorMessage = error.localizedDescription }
        }
    }

    func add(title: String, type: TaskTypeDto = .homework, subject: String = "") {
        let now = Date().iso8601String
        let due = Calendar.current.date(byAdding: .day, value: 1, to: .now) ?? .now
        let dueString = ISO8601DateFormatter().string(from: due)
        let task = TaskDto(
            id: UUID().uuidString.lowercased(),
            title: title,
            taskType: type,
            dueDate: dueString,
            reminderDate: now,
            subject: subject,
            importance: 3,
            notes: "",
            isCompleted: false,
            reminderEventId: nil,
            reminderCalendarId: nil,
            createdAt: now,
            phaseId: nil,
            coachExecutionData: nil,
            coachGoalId: nil,
            coachProposalId: nil,
            extraJson: "{}"
        )
        Task {
            do {
                try await service.upsertTask(task)
                await refresh()
            } catch { errorMessage = error.localizedDescription }
        }
    }

    private func recompute(now: Date = .now) {
        let filtered = tasks
            .filter { showCompleted || !$0.isCompleted }
            .filter { task in
                switch filter {
                case .all: true
                case .homework: task.taskType == .homework
                case .reading: task.taskType == .reading
                }
            }
            .sorted { dueDate($0) < dueDate($1) }

        let calendar = Calendar.current
        let week = calendar.date(byAdding: .day, value: 7, to: now) ?? now
        let month = calendar.date(byAdding: .month, value: 1, to: now) ?? week
        var overdue: [TaskDto] = []
        var thisWeek: [TaskDto] = []
        var thisMonth: [TaskDto] = []
        var later: [TaskDto] = []

        for task in filtered {
            let due = dueDate(task)
            if due < now {
                overdue.append(task)
            } else if due <= week {
                thisWeek.append(task)
            } else if due <= month {
                thisMonth.append(task)
            } else {
                later.append(task)
            }
        }

        sections = [
            TaskSection(id: "overdue", title: L10n.string("Overdue"), tasks: overdue),
            TaskSection(id: "week", title: L10n.string("Within 1 Week"), tasks: thisWeek),
            TaskSection(id: "month", title: L10n.string("Within 1 Month"), tasks: thisMonth),
            TaskSection(id: "later", title: L10n.string("Later"), tasks: later),
        ]
        .filter { !$0.tasks.isEmpty }
    }

    func formattedDueDate(_ task: TaskDto) -> String {
        dueDate(task).formatted(date: .abbreviated, time: .shortened)
    }

    private func dueDate(_ task: TaskDto) -> Date {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = fractional.date(from: task.dueDate) {
            return date
        }
        let standard = ISO8601DateFormatter()
        standard.formatOptions = [.withInternetDateTime]
        return standard.date(from: task.dueDate) ?? .distantFuture
    }
}

private extension Date {
    var iso8601String: String { ISO8601DateFormatter().string(from: self) }
}
