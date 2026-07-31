import SwiftUI

struct TodayP0View: View {
    @ObservedObject var viewModel: P0ViewModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                if let today = viewModel.today {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 160), spacing: 12)], spacing: 12) {
                        MetricCard(title: "Open Tasks", value: "\(today.openTaskCount)", symbol: "checklist")
                        MetricCard(
                            title: "Study Today",
                            value: L10n.format("%@ min", String(today.studyMinutes)),
                            symbol: "book.fill"
                        )
                        MetricCard(title: "Due Mistakes", value: "\(today.dueMistakeCount)", symbol: "arrow.triangle.2.circlepath")
                        MetricCard(
                            title: "Streak",
                            value: L10n.format("%@ days", String(today.streakDays)),
                            symbol: "flame.fill"
                        )
                    }
                    GroupBox("Deterministic suggestions") {
                        if today.suggestions.isEmpty {
                            Text("No suggestions yet.").foregroundStyle(.secondary)
                        } else {
                            VStack(alignment: .leading, spacing: 8) {
                                ForEach(today.suggestions, id: \.self) { suggestion in
                                    Label(suggestion, systemImage: "sparkles")
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                } else {
                    ContentUnavailableView("Loading Today", systemImage: "sun.max")
                }
            }
            .padding(24)
        }
        .toolbar {
            Button { Task { await viewModel.refresh() } } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .disabled(viewModel.isLoading)
        }
    }
}

struct TodayDetailView: View {
    let snapshot: TodaySnapshotDto?

    var body: some View {
        if let snapshot {
            DetailScrollView {
                DetailHeader(
                    title: L10n.string("Today"),
                    subtitle: L10n.string("StudyPulse overview"),
                    symbol: "sun.max"
                )
                DetailSection("Activity") {
                    DetailValueRow(label: "Open tasks", value: String(snapshot.openTaskCount))
                    DetailValueRow(label: "Completed", value: String(snapshot.completedTaskCount))
                    DetailValueRow(
                        label: "Study time",
                        value: L10n.format("%@ min", String(snapshot.studyMinutes))
                    )
                    DetailValueRow(
                        label: "Streak",
                        value: L10n.format("%@ days", String(snapshot.streakDays))
                    )
                }
                DetailSection("Focus") {
                    DetailValueRow(label: "Due mistakes", value: String(snapshot.dueMistakeCount))
                    DetailValueRow(label: "Upcoming exams", value: String(snapshot.upcomingExamIds.count))
                    DetailValueRow(
                        label: "Assigned time",
                        value: L10n.format("%@ min", String(snapshot.assignedInvestmentSeconds / 60))
                    )
                }
                if !snapshot.suggestions.isEmpty {
                    DetailSection("Suggestions") {
                        ForEach(snapshot.suggestions, id: \.self) { suggestion in
                            Label(suggestion, systemImage: "sparkles")
                        }
                    }
                }
            }
        } else {
            SelectionPlaceholderView(
                title: "Loading Today",
                symbol: "sun.max",
                message: "Rust is loading the workspace snapshot."
            )
        }
    }
}

private struct MetricCard: View {
    let title: String
    let value: String
    let symbol: String

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Image(systemName: symbol).font(.title3).foregroundStyle(.tint)
            Text(value).font(.title2.weight(.semibold))
            Text(LocalizedStringKey(title)).font(.caption).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(16)
        .background(.quaternary.opacity(0.55), in: RoundedRectangle(cornerRadius: 14))
    }
}

struct SubjectsGradesView: View {
    @ObservedObject var viewModel: P0ViewModel
    @State private var showingNewSubject = false

    var body: some View {
        List(selection: $viewModel.selectedSubjectID) {
            Section {
                ForEach(viewModel.subjects, id: \.id) { subject in
                    HStack {
                        Image(systemName: "book.closed.fill").foregroundStyle(.tint)
                        VStack(alignment: .leading) {
                            Text(subject.displayName)
                            Text(L10n.format(
                                "%@ · full score %@",
                                subject.name,
                                String(format: "%.0f", subject.fullScore)
                            ))
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Text(subject.enabled ? L10n.string("Enabled") : L10n.string("Disabled"))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    .tag(subject.id)
                    .contextMenu {
                        Button("Delete Subject", role: .destructive) {
                            Task { await viewModel.deleteSubject(subject.id) }
                        }
                    }
                }
            } header: {
                Text("Subjects")
            }
            Section("Recent Grades") {
                if viewModel.grades.isEmpty {
                    Text("No grades yet.").foregroundStyle(.secondary)
                } else {
                    ForEach(viewModel.grades, id: \.id) { grade in
                        HStack {
                            Text(grade.subject).font(.headline)
                            Spacer()
                            Text("\(grade.score, specifier: "%.1f")")
                            if let fullScore = grade.fullScore {
                                Text("/ \(fullScore, specifier: "%.0f")")
                                    .foregroundStyle(.secondary)
                            }
                            Text(grade.date.prefix(10)).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
        .toolbar {
            Button { showingNewSubject = true } label: {
                Label("New Subject", systemImage: "plus")
            }
            Button { Task { await viewModel.refresh() } } label: {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
        }
        .sheet(isPresented: $showingNewSubject) {
            NewSubjectSheet(viewModel: viewModel)
        }
    }
}

private struct NewSubjectSheet: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var viewModel: P0ViewModel
    @State private var name = ""
    @State private var displayName = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("New Subject").font(.title2.weight(.semibold))
            TextField("Internal name", text: $name)
            TextField("Display name", text: $displayName)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                Button("Create") {
                    Task {
                        await viewModel.addSubject(name: name, displayName: displayName)
                        dismiss()
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(width: 380)
    }
}

struct ExamsP0View: View {
    @ObservedObject var viewModel: P0ViewModel
    @State private var showingNewExam = false

    var body: some View {
        List(selection: $viewModel.selectedExamID) {
            ForEach(viewModel.exams, id: \.id) { exam in
                HStack {
                    Image(systemName: "calendar.badge.clock").foregroundStyle(.tint)
                    VStack(alignment: .leading) {
                        Text(exam.name).font(.headline)
                        Text([exam.subject, exam.examDate.prefix(10).description].filter { !$0.isEmpty }.joined(separator: " · "))
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text(L10n.format("Mastery %@%%", String(exam.masteryDegree)))
                        .font(.caption).foregroundStyle(.secondary)
                }
                .tag(exam.id)
                .contextMenu {
                    Button("Delete Exam", role: .destructive) {
                        Task { await viewModel.deleteExam(exam.id) }
                    }
                }
            }
        }
        .overlay {
            if viewModel.exams.isEmpty { ContentUnavailableView("No exams", systemImage: "calendar") }
        }
        .toolbar {
            Button { showingNewExam = true } label: { Label("New Exam", systemImage: "plus") }
            Button { Task { await viewModel.refresh() } } label: { Label("Refresh", systemImage: "arrow.clockwise") }
        }
        .sheet(isPresented: $showingNewExam) { NewExamSheet(viewModel: viewModel) }
    }
}

private struct NewExamSheet: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var viewModel: P0ViewModel
    @State private var name = ""
    @State private var subject = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("New Exam").font(.title2.weight(.semibold))
            TextField("Exam name", text: $name)
            TextField("Subject (optional)", text: $subject)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                Button("Create") {
                    Task {
                        await viewModel.addExam(name: name, subject: subject)
                        dismiss()
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(width: 380)
    }
}

struct MistakesP0View: View {
    @ObservedObject var viewModel: P0ViewModel

    var body: some View {
        List(selection: $viewModel.selectedMistakeID) {
            ForEach(viewModel.mistakes, id: \.id) { mistake in
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text(mistake.title).font(.headline)
                        Spacer()
                        if viewModel.dueMistakes.contains(where: { $0.id == mistake.id }) {
                            Label("Due", systemImage: "clock.badge.exclamationmark")
                                .font(.caption).foregroundStyle(.orange)
                        }
                    }
                    Text([mistake.subject, mistake.source].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.caption).foregroundStyle(.secondary)
                    HStack {
                        Text(L10n.format("Mastery %@%%", String(Int(mistake.masteryScore * 100))))
                            .font(.caption).foregroundStyle(.secondary)
                        Spacer()
                        ForEach([1, 3, 4, 5], id: \.self) { quality in
                            Button(qualityTitle(quality)) {
                                Task { await viewModel.reviewMistake(mistake.id, quality: Int64(quality)) }
                            }
                            .buttonStyle(.borderless)
                        }
                    }
                }
                .padding(.vertical, 5)
                .tag(mistake.id)
                .contextMenu {
                    Button("Delete Mistake", role: .destructive) {
                        Task { await viewModel.deleteMistake(mistake.id) }
                    }
                }
            }
        }
        .overlay {
            if viewModel.mistakes.isEmpty { ContentUnavailableView("No mistakes", systemImage: "checkmark.seal") }
        }
        .toolbar {
            Button { Task { await viewModel.refresh() } } label: { Label("Refresh", systemImage: "arrow.clockwise") }
        }
    }

    private func qualityTitle(_ quality: Int) -> String {
        switch quality {
        case 1: L10n.string("Again")
        case 3: L10n.string("Hard")
        case 4: L10n.string("Good")
        default: L10n.string("Easy")
        }
    }
}

struct StudyTimerP0View: View {
    @ObservedObject var viewModel: P0ViewModel
    @State private var intensity: SessionIntensityDto = .deepFocus

    var body: some View {
        VStack(spacing: 24) {
            Image(systemName: "timer").font(.system(size: 42)).foregroundStyle(.tint)
            Text(formatDuration(viewModel.timer.elapsedSeconds))
                .font(.system(size: 58, weight: .semibold, design: .rounded))
                .monospacedDigit()
            if viewModel.timer.status == .idle {
                Picker("Intensity", selection: $intensity) {
                    Text("Peak").tag(SessionIntensityDto.peak)
                    Text("Deep Focus").tag(SessionIntensityDto.deepFocus)
                    Text("Steady").tag(SessionIntensityDto.steady)
                    Text(L10n.string("Timer Light")).tag(SessionIntensityDto.light)
                    Text("Recovery").tag(SessionIntensityDto.recovery)
                }
                .pickerStyle(.segmented)
                .frame(maxWidth: 520)
                Button("Start", systemImage: "play.fill") {
                    Task { await viewModel.startTimer(intensity: intensity) }
                }
                .buttonStyle(.borderedProminent)
            } else {
                HStack {
                    if viewModel.timer.status == .running {
                        Button("Pause", systemImage: "pause.fill") { Task { await viewModel.pauseTimer() } }
                    } else {
                        Button("Resume", systemImage: "play.fill") { Task { await viewModel.resumeTimer() } }
                    }
                    Button("Finish", systemImage: "checkmark") { Task { await viewModel.finishTimer() } }
                        .buttonStyle(.borderedProminent)
                    Button("Cancel", role: .destructive) { Task { await viewModel.cancelTimer() } }
                }
            }
            Text(timerStatus(viewModel.timer.status))
                .font(.caption).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(32)
        .task {
            while !Task.isCancelled {
                await viewModel.pollTimer()
                try? await Task.sleep(nanoseconds: 1_000_000_000)
            }
        }
    }

    private func formatDuration(_ seconds: Int64) -> String {
        String(format: "%02lld:%02lld", seconds / 60, seconds % 60)
    }

    private func timerStatus(_ status: TimerStatusKindDto) -> String {
        switch status {
        case .idle: L10n.string("Ready")
        case .running: L10n.string("Running")
        case .paused: L10n.string("Paused")
        }
    }
}

struct TimeInvestmentP0View: View {
    @ObservedObject var viewModel: P0ViewModel
    @State private var showingNewProject = false

    var body: some View {
        List(selection: $viewModel.selectedInvestmentSubjectID) {
            Section("Projects") {
                ForEach(viewModel.investmentSubjects, id: \.id) { project in
                    Label(project.name, systemImage: project.symbolName)
                        .tag(project.id)
                }
            }
            Section("Time totals") {
                ForEach(viewModel.investmentSummary, id: \.targetId) { summary in
                    HStack {
                        Text(summary.targetId)
                        Spacer()
                        Text(L10n.format("%@ min", String(summary.totalSeconds / 60)))
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .overlay {
            if viewModel.investmentSubjects.isEmpty { ContentUnavailableView("No projects", systemImage: "chart.bar.xaxis") }
        }
        .toolbar {
            Button { showingNewProject = true } label: { Label("New Project", systemImage: "plus") }
            Button { Task { await viewModel.refresh() } } label: { Label("Refresh", systemImage: "arrow.clockwise") }
        }
        .sheet(isPresented: $showingNewProject) { NewInvestmentProjectSheet(viewModel: viewModel) }
    }
}

private struct NewInvestmentProjectSheet: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var viewModel: P0ViewModel
    @State private var name = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("New Time Investment Project").font(.title2.weight(.semibold))
            TextField("Project name", text: $name)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                Button("Create") {
                    Task {
                        await viewModel.addInvestmentSubject(name: name)
                        dismiss()
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(width: 420)
    }
}
