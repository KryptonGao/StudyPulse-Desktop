import SwiftUI

struct TasksView: View {
    @ObservedObject var viewModel: TasksViewModel
    @State private var isShowingAdd = false

    var body: some View {
        Group {
            if viewModel.isLoading && viewModel.tasks.isEmpty {
                ProgressView("Loading Tasks…")
            } else if viewModel.sections.isEmpty {
                ContentUnavailableView(
                    "No tasks",
                    systemImage: "checklist",
                    description: Text("Ask the Agent to create a task.")
                )
            } else {
                List(selection: $viewModel.selectedTaskID) {
                    ForEach(viewModel.sections) { section in
                        Section(section.title) {
                            ForEach(section.tasks) { task in
                                TaskRow(
                                    task: task,
                                    dueDate: viewModel.formattedDueDate(task),
                                    onToggle: { viewModel.toggle(task) },
                                    onDelete: { viewModel.delete(task) }
                                )
                                .tag(task.id)
                            }
                        }
                    }
                }
            }
        }
        .safeAreaInset(edge: .top) {
            HStack {
                Picker("Type", selection: $viewModel.filter) {
                    ForEach(TaskFilter.allCases) { filter in
                        Text(filter.title).tag(filter)
                    }
                }
                .pickerStyle(.segmented)
                .frame(maxWidth: 360)

                Toggle("Completed", isOn: $viewModel.showCompleted)
                    .toggleStyle(.checkbox)

                Spacer()

                Button {
                    isShowingAdd = true
                } label: {
                    Label("New Task", systemImage: "plus")
                }

                Button {
                    Task { await viewModel.refresh() }
                } label: {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(viewModel.isLoading)
            }
            .padding(12)
            .background(.bar)
        }
        .sheet(isPresented: $isShowingAdd) {
            NewTaskSheet(viewModel: viewModel)
        }
    }
}

private struct TaskRow: View {
    let task: TaskDto
    let dueDate: String
    let onToggle: () -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Button(action: onToggle) {
                Image(systemName: task.isCompleted ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(task.isCompleted ? .green : .secondary)
            }
            .buttonStyle(.plain)

            VStack(alignment: .leading, spacing: 5) {
                HStack {
                    Text(task.title)
                        .font(.headline)
                        .strikethrough(task.isCompleted)
                    Spacer()
                    Text(task.taskType.displayName)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                HStack(spacing: 10) {
                    if !task.subject.isEmpty {
                        Label(task.subject, systemImage: "book.closed")
                    }
                    Label(dueDate, systemImage: "calendar")
                    Label(L10n.format("%@/5", String(task.importance)), systemImage: "star.fill")
                }
                .font(.caption)
                .foregroundStyle(.secondary)

                if !task.notes.isEmpty {
                    Text(task.notes)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
        }
        .padding(.vertical, 6)
        .contextMenu {
            Button("Delete Task", role: .destructive, action: onDelete)
        }
    }
}

private struct NewTaskSheet: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var viewModel: TasksViewModel
    @State private var title = ""
    @State private var subject = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("New Task").font(.title2.weight(.semibold))
            TextField("Title", text: $title)
            TextField("Subject (optional)", text: $subject)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                Button("Create") {
                    viewModel.add(title: title, subject: subject)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(width: 380)
    }
}
