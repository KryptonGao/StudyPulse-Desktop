import SwiftUI
import SwiftStreamingMarkdown

struct SelectionPlaceholderView: View {
    let title: String
    let symbol: String
    let message: String

    var body: some View {
        ContentUnavailableView(
            LocalizedStringKey(title),
            systemImage: symbol,
            description: Text(LocalizedStringKey(message))
        )
    }
}

struct SubjectDetailView: View {
    let subject: SubjectDto
    let grades: [GradeDto]

    var body: some View {
        DetailScrollView {
            DetailHeader(
                title: subject.displayName,
                subtitle: subject.name,
                symbol: "book.closed.fill"
            )

            DetailSection("Subject") {
                DetailValueRow(
                    label: "Status",
                    value: subject.enabled ? L10n.string("Enabled") : L10n.string("Disabled")
                )
                DetailValueRow(label: "Full score", value: String(format: "%.0f", subject.fullScore))
            }

            DetailSection("Grades") {
                if grades.isEmpty {
                    Text("No grades recorded for this subject.")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(grades, id: \.id) { grade in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack {
                                Text(grade.examName.isEmpty ? L10n.string("Grade") : grade.examName)
                                    .font(.subheadline.weight(.medium))
                                Spacer()
                                Text(scoreText(grade))
                                    .font(.subheadline.weight(.semibold))
                            }
                            Text(grade.date.prefix(10))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        if grade.id != grades.last?.id { Divider() }
                    }
                }
            }
        }
    }

    private func scoreText(_ grade: GradeDto) -> String {
        if let fullScore = grade.fullScore {
            return String(format: "%.1f / %.0f", grade.score, fullScore)
        }
        return String(format: "%.1f", grade.score)
    }
}

struct ExamDetailView: View {
    let exam: ExamDto

    var body: some View {
        DetailScrollView {
            DetailHeader(
                title: exam.name,
                subtitle: exam.subject.isEmpty ? L10n.string("Exam") : exam.subject,
                symbol: "calendar.badge.clock"
            )

            DetailSection("Exam") {
                DetailValueRow(label: "Date", value: String(exam.examDate.prefix(10)))
                if let endDate = exam.examEndDate {
                    DetailValueRow(label: "Ends", value: String(endDate.prefix(10)))
                }
                DetailValueRow(label: "Importance", value: L10n.format("%@/5", String(exam.importance)))
                DetailValueRow(label: "Mastery", value: L10n.format("%@%%", String(exam.masteryDegree)))
                if !exam.locationSchool.isEmpty {
                    DetailValueRow(label: "School", value: exam.locationSchool)
                }
                if !exam.locationClassroom.isEmpty {
                    DetailValueRow(label: "Classroom", value: exam.locationClassroom)
                }
                if !exam.locationSeat.isEmpty {
                    DetailValueRow(label: "Seat", value: exam.locationSeat)
                }
            }

            if !exam.checklist.isEmpty {
                DetailSection("Checklist") {
                    ForEach(exam.checklist, id: \.id) { item in
                        Label(item.title, systemImage: item.isChecked ? "checkmark.circle.fill" : "circle")
                            .foregroundStyle(item.isChecked ? .green : .primary)
                    }
                }
            }

            if let review = exam.examReview {
                DetailSection("Review") {
                    DetailTextBlock(title: "What was tested", text: review.whatWasTested)
                    DetailTextBlock(title: "What went wrong", text: review.whatWentWrong)
                    DetailTextBlock(title: "What I learned", text: review.whatLearned)
                    DetailTextBlock(title: "Next strategy", text: review.nextStrategy)
                }
            }
        }
    }
}

struct MistakeDetailView: View {
    let mistake: MistakeNoteDto

    var body: some View {
        DetailScrollView {
            DetailHeader(
                title: mistake.title,
                subtitle: [mistake.subject, mistake.source].filter { !$0.isEmpty }.joined(separator: " · "),
                symbol: "exclamationmark.bubble"
            )

            DetailSection("Review state") {
                DetailValueRow(
                    label: "Mastery",
                    value: L10n.format("%@%%", String(Int(mistake.masteryScore * 100)))
                )
                DetailValueRow(label: "Exposures", value: String(mistake.exposureCount))
                DetailValueRow(label: "Difficulty", value: String(mistake.difficulty))
                if let reviewState = mistake.reviewState {
                    DetailValueRow(label: "Next review", value: String(reviewState.nextReviewDate.prefix(10)))
                    DetailValueRow(
                        label: "Interval",
                        value: L10n.format("%@ days", String(reviewState.intervalDays))
                    )
                }
                if !mistake.tags.isEmpty {
                    DetailValueRow(label: "Tags", value: mistake.tags.joined(separator: ", "))
                }
            }

            DetailTextBlock(title: "Original question", text: mistake.originalQuestion)
            DetailTextBlock(title: "Error reason", text: mistake.errorReason)
            DetailTextBlock(title: "Wrong solution", text: mistake.wrongSolution)
            DetailTextBlock(title: "Correct solution", text: mistake.correctSolution)

            if !mistake.questionImages.isEmpty || !mistake.reasonImages.isEmpty ||
                !mistake.wrongSolutionImages.isEmpty || !mistake.correctSolutionImages.isEmpty {
                DetailSection("Attachments") {
                    DetailValueRow(label: "Question images", value: String(mistake.questionImages.count))
                    DetailValueRow(label: "Reason images", value: String(mistake.reasonImages.count))
                    DetailValueRow(label: "Solution images", value: String(mistake.wrongSolutionImages.count + mistake.correctSolutionImages.count))
                    if let audio = mistake.audioFileName {
                        DetailValueRow(label: "Audio", value: audio)
                    }
                }
            }
        }
    }
}

struct TaskDetailView: View {
    let task: TaskDto

    var body: some View {
        DetailScrollView {
            DetailHeader(
                title: task.title,
                subtitle: task.subject.isEmpty ? task.taskType.displayName : task.subject,
                symbol: task.isCompleted ? "checkmark.circle.fill" : "checklist"
            )

            DetailSection("Task") {
                DetailValueRow(
                    label: "Status",
                    value: task.isCompleted ? L10n.string("Completed") : L10n.string("Open Task")
                )
                DetailValueRow(label: "Type", value: task.taskType.displayName)
                DetailValueRow(label: "Due", value: task.dueDate)
                DetailValueRow(label: "Importance", value: L10n.format("%@/5", String(task.importance)))
                DetailValueRow(label: "Created", value: task.createdAt)
            }

            if !task.notes.isEmpty {
                DetailTextBlock(title: "Notes", text: task.notes)
            }
        }
    }
}

struct InvestmentDetailView: View {
    let project: TimeInvestmentSubjectDto
    let summary: TimeInvestmentSummaryDto?

    var body: some View {
        DetailScrollView {
            DetailHeader(
                title: project.name,
                subtitle: project.isArchived ? L10n.string("Archived project") : L10n.string("Time Investment"),
                symbol: project.symbolName
            )

            DetailSection("Project") {
                DetailValueRow(label: "Theme", value: project.theme.displayName)
                DetailValueRow(label: "Start date", value: String(project.startDate.prefix(10)))
                DetailValueRow(label: "Created", value: String(project.createdAt.prefix(10)))
            }

            DetailSection("Time totals") {
                if let summary {
                    DetailValueRow(label: "Direct time", value: duration(summary.directSeconds))
                    DetailValueRow(label: "Total time", value: duration(summary.totalSeconds))
                    DetailValueRow(label: "Sessions", value: String(summary.sessionCount))
                } else {
                    Text("No study sessions recorded for this project.")
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private func duration(_ seconds: Int64) -> String {
        L10n.format("%@ min", String(seconds / 60))
    }
}

struct TimerDetailView: View {
    let timer: TimerSnapshotDto

    var body: some View {
        DetailScrollView {
            DetailHeader(title: L10n.string("Study Timer"), subtitle: status, symbol: "timer")
            DetailSection("Current session") {
                DetailValueRow(label: "Elapsed", value: duration(timer.elapsedSeconds))
                if timer.targetDurationSeconds > 0 {
                    DetailValueRow(label: "Target", value: duration(timer.targetDurationSeconds))
                }
                if let startedAt = timer.startedAt {
                    DetailValueRow(label: "Started", value: startedAt)
                }
            }
        }
    }

    private var status: String {
        switch timer.status {
        case .idle: L10n.string("Ready")
        case .running: L10n.string("Running")
        case .paused: L10n.string("Paused")
        }
    }

    private func duration(_ seconds: Int64) -> String {
        String(format: "%02lld:%02lld", seconds / 60, seconds % 60)
    }
}

struct LibraryDetailView: View {
    let file: FileEntryDto
    let matches: [SearchMatchDto]

    var body: some View {
        DetailScrollView {
            DetailHeader(
                title: file.relativePath,
                subtitle: file.isDirectory ? L10n.string("Folder") : L10n.string("Document"),
                symbol: file.isDirectory ? "folder" : "doc.text"
            )

            DetailSection("File") {
                DetailValueRow(label: "Size", value: ByteCountFormatter.string(fromByteCount: Int64(file.sizeBytes), countStyle: .file))
                if let modifiedAt = file.modifiedAt {
                    DetailValueRow(label: "Modified", value: modifiedAt)
                }
            }

            if !matches.isEmpty {
                DetailSection("Search matches") {
                    ForEach(Array(matches.enumerated()), id: \.offset) { _, match in
                        VStack(alignment: .leading, spacing: 4) {
                            if let line = match.lineNumber {
                                Text(L10n.format("Line %@", String(line)))
                                    .font(.caption.weight(.semibold))
                            }
                            Text(match.snippet).textSelection(.enabled)
                        }
                    }
                }
            }
        }
    }
}

struct DetailScrollView<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                content
            }
            .padding(24)
            .frame(maxWidth: 680, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Color(nsColor: .controlBackgroundColor))
    }
}

struct DetailHeader: View {
    let title: String
    let subtitle: String
    let symbol: String

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            Image(systemName: symbol)
                .font(.title2)
                .foregroundStyle(.tint)
                .frame(width: 40, height: 40)
                .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.title2.weight(.semibold))
                    .textSelection(.enabled)
                if !subtitle.isEmpty {
                    Text(subtitle)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.bottom, 4)
    }
}

struct DetailSection<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(LocalizedStringKey(title))
                .font(.headline)
            VStack(alignment: .leading, spacing: 8) {
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(14)
            .background(.quaternary.opacity(0.45), in: RoundedRectangle(cornerRadius: 12))
        }
    }
}

struct DetailValueRow: View {
    let label: String
    let value: String

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Text(LocalizedStringKey(label))
                .foregroundStyle(.secondary)
                .frame(width: 110, alignment: .leading)
            Text(value)
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
    }
}

private struct DetailTextBlock: View {
    let title: String
    let text: String

    var body: some View {
        if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            DetailSection(title) {
                MarkdownTextView(text: text)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

struct MarkdownTextView: View {
    let text: String
    @StateObject private var source: AgentMarkdownSource

    init(text: String) {
        self.text = text
        _source = StateObject(wrappedValue: AgentMarkdownSource(text: text))
    }

    var body: some View {
        StreamedMarkdownView(
            source: source,
            config: .default.withShouldAnimateText(value: false)
        )
        .frame(maxWidth: .infinity, alignment: .leading)
        .onChange(of: text) { _, newText in
            source.update(text: newText)
        }
    }
}
