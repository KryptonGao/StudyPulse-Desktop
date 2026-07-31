import SwiftUI
import SwiftStreamingMarkdown

struct AgentView: View {
    @ObservedObject var viewModel: AgentViewModel
    let onChooseSourceFiles: () -> Void
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @FocusState private var isPromptFocused: Bool
    @State private var notebookTitle = ""

    var body: some View {
        VStack(spacing: 0) {
            notebookHeader

            ScrollViewReader { proxy in
                ScrollView {
                    conversation
                        .frame(maxWidth: 860)
                        .frame(maxWidth: .infinity)
                        .padding(.horizontal, 32)
                        .padding(.vertical, 28)
                }
                .onChange(of: viewModel.selectedMessages) {
                    if let lastMessage = viewModel.selectedMessages.last {
                        withAnimation(.easeOut(duration: 0.18)) {
                            proxy.scrollTo(lastMessage.id, anchor: .bottom)
                        }
                    }
                }
            }

            composer
                .frame(maxWidth: 860)
                .frame(maxWidth: .infinity)
                .padding(.horizontal, 32)
                .padding(.bottom, 28)
        }
        .background(Color(nsColor: .textBackgroundColor))
        .sheet(isPresented: $viewModel.isShowingSourcePicker) {
            SourcePickerSheet(
                viewModel: viewModel,
                onChooseSourceFiles: onChooseSourceFiles
            )
        }
        .sheet(item: $viewModel.pendingConfirmation) { confirmation in
            ConfirmationSheet(
                confirmation: confirmation,
                onDecision: viewModel.resolveConfirmation(allow:)
            )
        }
        .sheet(item: $viewModel.pendingInput) { input in
            AgentInputSheet(input: input, onSubmit: viewModel.resolveInput(answer:))
        }
        .onAppear {
            notebookTitle = viewModel.selectedNotebook?.title ?? ""
        }
        .onChange(of: viewModel.selectedNotebookID) {
            notebookTitle = viewModel.selectedNotebook?.title ?? ""
        }
    }

    private var notebookHeader: some View {
        HStack(spacing: 14) {
            Image(systemName: "book.closed.fill")
                .font(.title3)
                .foregroundStyle(.tint)
                .frame(width: 34, height: 34)
                .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 9))

            VStack(alignment: .leading, spacing: 2) {
                TextField("Notebook title", text: $notebookTitle)
                    .textFieldStyle(.plain)
                    .font(.headline)
                    .disabled(viewModel.isRunning)
                    .onChange(of: notebookTitle) {
                        if !notebookTitle
                            .trimmingCharacters(in: .whitespacesAndNewlines)
                            .isEmpty
                        {
                            viewModel.renameSelectedNotebook(notebookTitle)
                        }
                    }
                    .onSubmit {
                        viewModel.renameSelectedNotebook(notebookTitle)
                        notebookTitle = viewModel.selectedNotebook?.title ?? ""
                    }

                Text(sourceSummary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Button {
                viewModel.isShowingSourcePicker = true
            } label: {
                Label("Sources", systemImage: "doc.on.doc")
            }
            .disabled(viewModel.isRunning)

            modePicker

            cloudAccountControl
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .background {
            if reduceTransparency {
                Color(nsColor: .windowBackgroundColor)
            } else {
                Rectangle().fill(.regularMaterial)
            }
        }
        .overlay(alignment: .bottom) {
            Divider()
        }
    }

    @ViewBuilder
    private var cloudAccountControl: some View {
        if let account = viewModel.cloudAccount {
            Menu {
                Text(account.email)
                        Text(L10n.format("%@ · Cloud AI", account.planName))
                Divider()
                Button("Sign Out", action: viewModel.signOut)
                    .disabled(viewModel.isRunning || viewModel.isAuthenticating)
            } label: {
                Label(account.planName, systemImage: "checkmark.icloud")
            }
        } else if viewModel.isAuthenticating {
            ProgressView()
                .controlSize(.small)
                .frame(width: 76)
        } else {
            Button(action: viewModel.signIn) {
                Label("Sign In", systemImage: "icloud")
            }
            .buttonStyle(.borderedProminent)
        }
    }

    @ViewBuilder
    private var conversation: some View {
        if viewModel.selectedMessages.isEmpty {
            VStack(spacing: 18) {
                Spacer(minLength: 70)

                Image(systemName: "book.pages")
                    .font(.system(size: 36, weight: .light))
                    .foregroundStyle(.secondary)

                Text("What would you like to explore?")
                    .font(.system(.largeTitle, design: .rounded, weight: .medium))
                    .multilineTextAlignment(.center)

                Text(emptyStateDescription)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 520)

                if viewModel.selectedSourcePaths.isEmpty {
                    Button {
                        viewModel.isShowingSourcePicker = true
                    } label: {
                        Label("Add Sources", systemImage: "plus")
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(viewModel.isRunning)
                }

                Spacer(minLength: 50)
            }
            .frame(minHeight: 380)
        } else {
            LazyVStack(spacing: 18) {
                ForEach(viewModel.selectedMessages) { message in
                    AgentChatBubble(message: message)
                        .id(message.id)
                }

                if viewModel.isRunning,
                   viewModel.selectedMessages.last?.role == .user
                {
                    AgentTypingIndicator(status: viewModel.currentActivity ?? L10n.string("Thinking"))
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }

    private var composer: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let error = viewModel.errorMessage {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            VStack(spacing: 0) {
                TextField(
                    "Ask about this notebook…",
                    text: $viewModel.goal,
                    axis: .vertical
                )
                .focused($isPromptFocused)
                .lineLimit(2 ... 6)
                .textFieldStyle(.plain)
                .font(.body)
                .padding(.horizontal, 18)
                .padding(.top, 16)
                .padding(.bottom, 14)
                .onSubmit {
                    if !viewModel.isRunning {
                        viewModel.start()
                    }
                }

                HStack(spacing: 10) {
                    modePicker
                        .controlSize(.small)

                    Button {
                        viewModel.isShowingSourcePicker = true
                    } label: {
                        Label(sourceButtonTitle, systemImage: "paperclip")
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(.secondary)
                    .disabled(viewModel.isRunning)

                    Spacer()

                    if viewModel.isRunning {
                        HStack(spacing: 7) {
                            ProgressView()
                                .controlSize(.small)
                            Text(viewModel.currentActivity ?? viewModel.status?.displayName ?? L10n.string("Running"))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }

                        Button(action: viewModel.cancel) {
                            Image(systemName: "stop.fill")
                                .frame(width: 30, height: 30)
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(.red)
                        .clipShape(Circle())
                        .help("Stop Agent")
                    } else {
                        Button(action: viewModel.start) {
                            Image(systemName: "arrow.up")
                                .font(.body.weight(.semibold))
                                .frame(width: 30, height: 30)
                        }
                        .buttonStyle(.borderedProminent)
                        .clipShape(Circle())
                        .disabled(
                            !viewModel.isCloudConnected
                                || viewModel.selectedNotebook == nil
                                || viewModel.goal
                                    .trimmingCharacters(in: .whitespacesAndNewlines)
                                    .isEmpty
                        )
                        .help("Run Agent")
                    }
                }
                .padding(.horizontal, 18)
                .padding(.bottom, 14)
            }
            .background {
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .fill(reduceTransparency ? AnyShapeStyle(.background) : AnyShapeStyle(.regularMaterial))
                    .shadow(color: .black.opacity(0.08), radius: 18, y: 8)
            }
            .overlay {
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .stroke(.quaternary, lineWidth: 1)
            }
        }
    }

    private var modePicker: some View {
        Menu {
            ForEach(AgentModeDto.allCases) { mode in
                Button {
                    viewModel.mode = mode
                } label: {
                    Label(mode.title, systemImage: mode.symbol)
                }
                .disabled(viewModel.isRunning)
            }
        } label: {
            Label(viewModel.mode.title, systemImage: viewModel.mode.symbol)
        }
        .menuStyle(.borderlessButton)
        .help("Agent mode")
    }

    private var sourceSummary: String {
        switch viewModel.selectedSourcePaths.count {
        case 0: L10n.string("No sources selected")
        case 1: L10n.string("1 source")
        default: L10n.format("%@ sources", String(viewModel.selectedSourcePaths.count))
        }
    }

    private var sourceButtonTitle: String {
        viewModel.selectedSourcePaths.isEmpty
            ? L10n.string("Add sources")
            : L10n.format("%@ sources", String(viewModel.selectedSourcePaths.count))
    }

    private var emptyStateDescription: String {
        if !viewModel.isCloudConnected {
            return L10n.string("Sign in to Cloud AI, then ask a question. Your Workspace and notebook sources stay local until the Agent needs them.")
        }
        if viewModel.selectedSourcePaths.isEmpty {
            return L10n.string("Add files from your Library as sources, or ask a question without Library context.")
        }
        return L10n.string("The Agent will reason only with this notebook’s selected Library sources.")
    }
}

private struct AgentInputSheet: View {
    let input: PendingAgentInput
    let onSubmit: (String) -> Void
    @State private var answer = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label("Agent needs your input", systemImage: "questionmark.bubble")
                .font(.headline)
            Text(input.prompt)
                .font(.body)
            if !input.options.isEmpty {
                ForEach(input.options, id: \.self) { option in
                    Button(option) {
                        answer = option
                    }
                    .buttonStyle(.bordered)
                }
            }
            TextField("Your answer", text: $answer, axis: .vertical)
                .textFieldStyle(.roundedBorder)
            HStack {
                Spacer()
                Button("Continue") { onSubmit(answer) }
                    .buttonStyle(.borderedProminent)
                    .disabled(answer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(width: 420)
    }
}

struct AgentInspectorView: View {
    @ObservedObject var viewModel: AgentViewModel

    var body: some View {
        VStack(spacing: 0) {
            sources
            Divider()
            activity
        }
        .background(Color(nsColor: .controlBackgroundColor))
    }

    private var sources: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Sources")
                        .font(.headline)
                    Text("Available only to this notebook")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button {
                    viewModel.isShowingSourcePicker = true
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
                .disabled(viewModel.isRunning)
                .help("Manage Sources")
            }

            if viewModel.selectedSources.isEmpty {
                Text("No files selected")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 72)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(viewModel.selectedSources, id: \.relativePath) { file in
                            Label {
                                Text(file.relativePath)
                                    .lineLimit(2)
                                    .truncationMode(.middle)
                            } icon: {
                                Image(systemName: "doc.text")
                                    .foregroundStyle(.secondary)
                            }
                            .font(.caption)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
                .frame(maxHeight: 180)
            }
        }
        .padding(16)
    }

    private var activity: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Activity")
                .font(.headline)
                .padding(.horizontal, 16)
                .padding(.top, 14)

            AgentTimelineView(viewModel: viewModel)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct AgentChatBubble: View {
    let message: AgentChatMessage

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            if message.role == .user {
                Spacer(minLength: 96)

                Text(message.content)
                    .textSelection(.enabled)
                    .lineSpacing(3)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 11)
                    .background(.tint.opacity(0.14), in: RoundedRectangle(
                        cornerRadius: 17,
                        style: .continuous
                    ))
            } else {
                Image(systemName: "sparkles")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.tint)
                    .frame(width: 30, height: 30)
                    .background(.tint.opacity(0.10), in: Circle())

                VStack(alignment: .leading, spacing: 8) {
                    Text("StudyPulse Agent")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)

                    AgentMarkdownText(text: message.content)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(16)
                .background(.quaternary.opacity(0.55), in: RoundedRectangle(
                    cornerRadius: 18,
                    style: .continuous
                ))

                Spacer(minLength: 44)
            }
        }
        .frame(maxWidth: .infinity)
    }
}

private struct AgentMarkdownText: View {
    let text: String
    @StateObject private var source: AgentMarkdownSource

    init(text: String) {
        self.text = text
        _source = StateObject(wrappedValue: AgentMarkdownSource(text: text))
    }

    var body: some View {
        StreamedMarkdownView(
            source: source,
            config: .default.withShouldAnimateText(value: true)
        )
        .onChange(of: text) { _, newText in
            source.update(text: newText)
        }
    }
}

private struct AgentTypingIndicator: View {
    let status: String

    var body: some View {
        HStack(spacing: 10) {
            ProgressView()
                .controlSize(.small)
            Text(status)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.leading, 42)
        .accessibilityLabel(L10n.format("StudyPulse Agent %@", status))
    }
}

private struct SourcePickerSheet: View {
    @ObservedObject var viewModel: AgentViewModel
    let onChooseSourceFiles: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Notebook Sources")
                        .font(.title2.weight(.semibold))
                    Text("The Agent can read only the files selected here.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button {
                    onChooseSourceFiles()
                } label: {
                    Label("Choose Files…", systemImage: "plus")
                }
                .disabled(viewModel.isImportingSources)

                Button("Done") {
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(20)

            Divider()

            if viewModel.selectableSources.isEmpty {
                VStack(spacing: 14) {
                    Image(systemName: "doc.badge.plus")
                        .font(.system(size: 42, weight: .light))
                        .foregroundStyle(.secondary)
                    Text("No source files")
                        .font(.title2.weight(.semibold))
                    Text("Choose text files from your Mac. They’ll be copied into this Workspace and selected for the notebook.")
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 440)
                    Button {
                        onChooseSourceFiles()
                    } label: {
                        Label("Choose Files…", systemImage: "plus")
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(viewModel.isImportingSources)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(filteredSources, id: \.relativePath) { file in
                    Button {
                        viewModel.toggleSource(file.relativePath)
                    } label: {
                        HStack(spacing: 12) {
                            Image(systemName: viewModel.isSourceSelected(file.relativePath)
                                ? "checkmark.circle.fill"
                                : "circle")
                                .foregroundStyle(
                                    viewModel.isSourceSelected(file.relativePath)
                                        ? AnyShapeStyle(.tint)
                                        : AnyShapeStyle(.tertiary)
                                )

                            VStack(alignment: .leading, spacing: 3) {
                                Text(URL(fileURLWithPath: file.relativePath).lastPathComponent)
                                    .font(.body)
                                Text(file.relativePath)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                            }

                            Spacer()

                            Text(ByteCountFormatter.string(
                                fromByteCount: Int64(file.sizeBytes),
                                countStyle: .file
                            ))
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                .searchable(text: $query, prompt: "Search sources")
            }
        }
        .overlay {
            if viewModel.isImportingSources {
                ProgressView("Adding sources…")
                    .padding(18)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
            }
        }
        .frame(minWidth: 620, minHeight: 520)
    }

    private var filteredSources: [FileEntryDto] {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty else { return viewModel.selectableSources }
        return viewModel.selectableSources.filter {
            $0.relativePath.localizedCaseInsensitiveContains(needle)
        }
    }
}

private struct ConfirmationSheet: View {
    let confirmation: PendingConfirmation
    let onDecision: (Bool) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label("Confirmation required", systemImage: "hand.raised.fill")
                .font(.title2.weight(.semibold))

            LabeledContent("Tool", value: confirmation.toolName)
            LabeledContent("Permission", value: confirmation.permission.displayName)

            Text(confirmation.preview)
                .foregroundStyle(.secondary)

            if confirmation.permission == .execute {
                Text("The Agent requested local Python execution. StudyPulse validates the request and will not run it until you allow it. This is not Docker-isolated.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            } else {
                Text("The Agent requested this operation. StudyPulse validates it locally and will not run it until you allow it.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }

            GroupBox("Arguments") {
                ScrollView {
                    Text(confirmation.payloadJSON)
                        .font(.system(.caption, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(minHeight: 100, maxHeight: 220)
            }

            Text("This approval applies only to this operation.")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack {
                Spacer()
                Button("Deny", role: .cancel) {
                    onDecision(false)
                }
                Button("Allow once") {
                    onDecision(true)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(24)
        .frame(width: 520)
    }
}
