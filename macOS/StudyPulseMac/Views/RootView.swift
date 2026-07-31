import Foundation
import SwiftUI

struct RootView: View {
    @ObservedObject var appModel: AppViewModel
    @ObservedObject private var p0: P0ViewModel
    @ObservedObject private var tasks: TasksViewModel
    @ObservedObject private var library: LibraryViewModel
    @State private var notebookToDelete: AgentNotebook?

    init(appModel: AppViewModel) {
        self.appModel = appModel
        _p0 = ObservedObject(wrappedValue: appModel.p0)
        _tasks = ObservedObject(wrappedValue: appModel.tasks)
        _library = ObservedObject(wrappedValue: appModel.library)
    }

    var body: some View {
        Group {
            if appModel.workspace.workspace == nil {
                WorkspaceWelcomeView(appModel: appModel)
            } else {
                workspaceContent
            }
        }
        .sheet(isPresented: $appModel.isShowingBackupImport) {
            BackupImportSheet(appModel: appModel)
        }
        .alert(
            "Workspace Error",
            isPresented: Binding(
                get: { appModel.workspace.errorMessage != nil },
                set: { if !$0 { appModel.workspace.errorMessage = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(appModel.workspace.errorMessage ?? "")
        }
        .alert(
            "Core Error",
            isPresented: Binding(
                get: { p0.errorMessage != nil },
                set: { if !$0 { p0.errorMessage = nil } }
            )
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(p0.errorMessage ?? "")
        }
        .alert(
            "Delete Notebook?",
            isPresented: Binding(
                get: { notebookToDelete != nil },
                set: { if !$0 { notebookToDelete = nil } }
            ),
            presenting: notebookToDelete
        ) { notebook in
            Button("Delete", role: .destructive) {
                appModel.agent.deleteNotebook(notebook.id)
                notebookToDelete = nil
            }
            Button("Cancel", role: .cancel) {
                notebookToDelete = nil
            }
        } message: { notebook in
            Text(L10n.format(
                "“%@” and its saved response will be removed. Source files are not deleted.",
                notebook.title
            ))
        }
    }

    private var workspaceContent: some View {
        NavigationSplitView {
            sidebar
                .navigationTitle("StudyPulse")
                .navigationSplitViewColumnWidth(min: 250, ideal: 280, max: 340)
                .background(sidebarBackground.ignoresSafeArea())
                .overlay(alignment: .trailing) {
                    Rectangle()
                        .fill(Color(nsColor: .separatorColor))
                        .frame(width: 1)
                        .ignoresSafeArea()
                }
                .safeAreaInset(edge: .bottom, spacing: 0) {
                    workspaceFooter
                }
        } content: {
            content
                .navigationTitle(appModel.destination.title)
                .toolbar {
                    ToolbarItemGroup {
                        Button {
                            appModel.importBackup()
                        } label: {
                            Label("Import Backup", systemImage: "square.and.arrow.down")
                        }

                        Button {
                            appModel.exportBackup()
                        } label: {
                            Label("Export Backup", systemImage: "square.and.arrow.up")
                        }

                        Menu {
                            Button("New Workspace…", action: appModel.createWorkspace)
                            Button("Open Workspace…", action: appModel.openWorkspace)
                            Divider()
                            Button("Close Workspace", action: appModel.closeWorkspace)
                        } label: {
                            Label("Workspace", systemImage: "folder")
                        }
                    }
                }
        } detail: {
            detail
        }
        .navigationSplitViewStyle(.balanced)
        .background(sidebarBackground.ignoresSafeArea())
    }

    private var sidebar: some View {
        List {
            Section {
                ForEach(AppDestination.allCases) { destination in
                    Button {
                        appModel.destination = destination
                    } label: {
                        Label(destination.title, systemImage: destination.symbol)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .listRowBackground(
                        appModel.destination == destination
                            ? Color.accentColor.opacity(0.14)
                            : Color.clear
                    )
                }
            }

            if appModel.destination == .agent {
                Section {
                    ForEach(appModel.agent.notebooks) { notebook in
                        Button {
                            appModel.agent.selectNotebook(notebook.id)
                        } label: {
                            HStack(spacing: 9) {
                                Image(systemName: "book.closed")
                                    .foregroundStyle(.secondary)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(notebook.title)
                                        .lineLimit(1)
                                    Text(notebook.sourcePaths.isEmpty
                                        ? L10n.string("No sources")
                                        : L10n.format("%@ sources", String(notebook.sourcePaths.count)))
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(appModel.agent.isRunning)
                        .listRowBackground(
                            appModel.agent.selectedNotebookID == notebook.id
                                ? Color.accentColor.opacity(0.14)
                                : Color.clear
                        )
                        .contextMenu {
                            Button("Delete Notebook", role: .destructive) {
                                notebookToDelete = notebook
                            }
                            .disabled(appModel.agent.isRunning)
                        }
                    }
                } header: {
                    HStack {
                        Text("Notebooks")
                        Spacer()
                        Button(action: appModel.agent.createNotebook) {
                            Image(systemName: "plus")
                        }
                        .buttonStyle(.borderless)
                        .disabled(appModel.agent.isRunning)
                        .help("New Notebook")
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .background(sidebarBackground)
    }

    @ViewBuilder
    private var content: some View {
        switch appModel.destination {
        case .today:
            TodayP0View(viewModel: p0)
        case .agent:
            AgentView(
                viewModel: appModel.agent,
                onChooseSourceFiles: appModel.chooseAgentSourceFiles
            )
        case .tasks:
            TasksView(viewModel: tasks)
        case .subjects:
            SubjectsGradesView(viewModel: p0)
        case .exams:
            ExamsP0View(viewModel: p0)
        case .mistakes:
            MistakesP0View(viewModel: p0)
        case .timer:
            StudyTimerP0View(viewModel: p0)
        case .timeInvestment:
            TimeInvestmentP0View(viewModel: p0)
        case .library:
            LibraryView(viewModel: library)
        }
    }

    @ViewBuilder
    private var detail: some View {
        if appModel.destination == .agent {
            AgentInspectorView(viewModel: appModel.agent)
                .navigationTitle("Notebook")
        } else if appModel.destination == .today {
            TodayDetailView(snapshot: p0.today)
        } else {
            switch appModel.destination {
            case .subjects:
                if let subject = p0.selectedSubject {
                    SubjectDetailView(
                        subject: subject,
                        grades: p0.grades.filter { $0.subject == subject.name || $0.subject == subject.displayName }
                    )
                } else {
                    SelectionPlaceholderView(
                        title: "Select a subject",
                        symbol: "book.closed",
                        message: "Choose a subject in the middle column to view its grades and settings."
                    )
                }
            case .exams:
                if let exam = p0.selectedExam {
                    ExamDetailView(exam: exam)
                } else {
                    SelectionPlaceholderView(
                        title: "Select an exam",
                        symbol: "calendar.badge.clock",
                        message: "Choose an exam in the middle column to view its schedule, checklist and review."
                    )
                }
            case .mistakes:
                if let mistake = p0.selectedMistake {
                    MistakeDetailView(mistake: mistake)
                } else {
                    SelectionPlaceholderView(
                        title: "Select a mistake",
                        symbol: "exclamationmark.bubble",
                        message: "Choose a mistake in the middle column to view the question, solution and review state."
                    )
                }
            case .tasks:
                if let task = tasks.selectedTask {
                    TaskDetailView(task: task)
                } else {
                    SelectionPlaceholderView(
                        title: "Select a task",
                        symbol: "checklist",
                        message: "Choose a task in the middle column to view its details."
                    )
                }
            case .timer:
                TimerDetailView(timer: p0.timer)
            case .timeInvestment:
                if let project = p0.selectedInvestmentSubject {
                    InvestmentDetailView(
                        project: project,
                        summary: p0.investmentSummary.first { $0.targetId == project.id }
                    )
                } else {
                    SelectionPlaceholderView(
                        title: "Select a project",
                        symbol: "chart.bar.xaxis",
                        message: "Choose a time investment project in the middle column to view its totals."
                    )
                }
            case .library:
                if let file = library.selectedFile {
                    LibraryDetailView(file: file, matches: library.selectedMatches)
                } else {
                    SelectionPlaceholderView(
                        title: "Select a document",
                        symbol: "books.vertical",
                        message: "Choose a document in the middle column to view its metadata and search matches."
                    )
                }
            case .today, .agent:
                EmptyView()
            }
        }
    }

    private var workspaceFooter: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(workspaceName)
                .font(.caption.weight(.semibold))
                .lineLimit(1)
            Text(appModel.workspace.workspace?.rootPath ?? "")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(sidebarBackground)
        .overlay(alignment: .top) {
            Divider()
        }
    }

    private var sidebarBackground: Color {
        Color(nsColor: .windowBackgroundColor)
    }

    private var workspaceName: String {
        guard let path = appModel.workspace.workspace?.rootPath else { return L10n.string("Workspace") }
        return URL(fileURLWithPath: path).lastPathComponent
    }
}
