import AppKit
import SwiftUI

enum StudyPulseAppearance: String, CaseIterable, Identifiable {
    case system
    case light
    case dark

    var id: String { rawValue }

    var title: String {
        switch self {
        case .system: L10n.string("System")
        case .light: L10n.string("Light")
        case .dark: L10n.string("Dark")
        }
    }

    var symbol: String {
        switch self {
        case .system: "circle.lefthalf.filled"
        case .light: "sun.max"
        case .dark: "moon"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}

struct SettingsView: View {
    @ObservedObject var appModel: AppViewModel
    @AppStorage("StudyPulse.appearance") private var appearanceRawValue = StudyPulseAppearance.system.rawValue

    private var appearance: Binding<StudyPulseAppearance> {
        Binding(
            get: { StudyPulseAppearance(rawValue: appearanceRawValue) ?? .system },
            set: { appearanceRawValue = $0.rawValue }
        )
    }

    var body: some View {
        TabView {
            GeneralSettingsView(appearance: appearance)
                .tabItem {
                    Label("General", systemImage: "gearshape")
                }

            CloudAISettingsView(agent: appModel.agent)
                .tabItem {
                    Label("Cloud AI", systemImage: "sparkles")
                }

            WorkspaceSettingsView(appModel: appModel)
                .tabItem {
                    Label("Workspace", systemImage: "folder")
                }

            AboutSettingsView()
                .tabItem {
                    Label("About", systemImage: "info.circle")
                }
        }
        .padding(20)
        .frame(minWidth: 600, minHeight: 420)
    }
}

private struct GeneralSettingsView: View {
    @Binding var appearance: StudyPulseAppearance
    @AppStorage("StudyPulse.language") private var languageRawValue = AppLanguage.system.rawValue

    private var language: Binding<AppLanguage> {
        Binding(
            get: { AppLanguage(rawValue: languageRawValue) ?? .system },
            set: { languageRawValue = $0.rawValue }
        )
    }

    var body: some View {
        Form {
            Section {
                Picker("Appearance", selection: $appearance) {
                    ForEach(StudyPulseAppearance.allCases) { option in
                        Label(option.title, systemImage: option.symbol)
                            .tag(option)
                    }
                }
                .pickerStyle(.segmented)

                Picker("Language", selection: language) {
                    ForEach(AppLanguage.allCases) { option in
                        Text(option.title).tag(option)
                    }
                }
            } header: {
                SettingsSectionHeader(
                    title: "Interface",
                    message: "Choose how StudyPulse looks across its windows."
                )
            }

            Section {
                SettingsShortcutRow(title: "Open Settings", shortcut: "⌘,")
                SettingsShortcutRow(title: "Open Workspace", shortcut: "⌘O")
                SettingsShortcutRow(title: "New Workspace", shortcut: "⇧⌘N")
            } header: {
                SettingsSectionHeader(
                    title: "Keyboard Shortcuts",
                    message: "Keep your most common workspace actions close at hand."
                )
            }

            if language.wrappedValue != .system {
                Section {
                    Text("Restart StudyPulse to apply language changes.")
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .formStyle(.grouped)
    }
}

private struct CloudAISettingsView: View {
    @ObservedObject var agent: AgentViewModel
    @State private var apiKey = ""
    @State private var baseURL = "https://api.openai.com/v1"
    @State private var model = "gpt-4o-mini"

    var body: some View {
        Form {
            Section {
                HStack(spacing: 12) {
                    Image(systemName: agent.isAIConfigured ? "checkmark.circle.fill" : "person.crop.circle")
                        .font(.system(size: 24))
                        .foregroundStyle(agent.isAIConfigured ? .green : .secondary)

                    VStack(alignment: .leading, spacing: 3) {
                        Text(agent.isAIConfigured ? L10n.string("Connected") : L10n.string("Not connected"))
                            .font(.headline)
                        Text(agent.isBYOKConnected
                            ? L10n.string("BYOK is active for Agent conversations.")
                            : agent.isCloudConnected
                                ? L10n.string("Cloud AI is ready for Agent conversations.")
                                : L10n.string("Sign in or configure BYOK to use the Agent."))
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    if agent.isAuthenticating || agent.isConfiguringBYOK {
                        ProgressView()
                            .controlSize(.small)
                    } else if agent.isCloudConnected {
                        Button("Sign Out") {
                            agent.signOut()
                        }
                    } else {
                        Button("Sign In") {
                            agent.signIn()
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }
                .padding(.vertical, 4)
            } header: {
                SettingsSectionHeader(
                    title: "Account",
                    message: "Your session is stored securely in the macOS Keychain."
                )
            }

            if let account = agent.cloudAccount {
                Section("Plan") {
                    LabeledContent("Account", value: account.email)
                    LabeledContent("Plan", value: account.planName)
                    if !account.availableModels.isEmpty {
                        LabeledContent("Available Models", value: account.availableModels.joined(separator: ", "))
                    }
                    if let expiration = account.membershipExpiresAt {
                        LabeledContent("Membership", value: expiration)
                    }
                }
            }

            Section {
                SecureField("API Key", text: $apiKey)
                    .textContentType(.password)
                TextField("Base URL", text: $baseURL)
                    .textContentType(.URL)
                TextField("Model", text: $model)

                Text("Use any OpenAI-compatible endpoint. The API key is stored only in the macOS Keychain and is sent directly to this endpoint.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)

                HStack {
                    Button("Save & Use") {
                        agent.saveBYOK(apiKey: apiKey, baseURL: baseURL, model: model)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(agent.isRunning || agent.isAuthenticating || agent.isConfiguringBYOK)

                    if agent.hasSavedBYOK && !agent.isBYOKConnected {
                        Button("Use Saved Key") {
                            agent.useSavedBYOK()
                        }
                    }

                    if agent.hasSavedBYOK {
                        Button("Remove", role: .destructive) {
                            agent.removeBYOK()
                        }
                    }
                }
            } header: {
                SettingsSectionHeader(
                    title: "BYOK (OpenAI-compatible)",
                    message: "Bring your own API key for OpenAI and compatible providers."
                )
            }

            Section {
                Text("The active AI provider can read selected workspace sources and create tasks when you approve an action. You will be asked before it writes to your workspace.")
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } header: {
                Text("Privacy & Permissions")
            }
        }
        .formStyle(.grouped)
        .onAppear {
            if let config = agent.byokConfig {
                baseURL = config.baseUrl
                model = config.model
            }
        }
        .onChange(of: agent.byokConfig) { _, config in
            guard let config else { return }
            baseURL = config.baseUrl
            model = config.model
        }
    }
}

private struct WorkspaceSettingsView: View {
    @ObservedObject var appModel: AppViewModel

    var body: some View {
        Form {
            Section {
                if let workspace = appModel.workspace.workspace {
                    LabeledContent("Location") {
                        Text(workspace.rootPath)
                            .font(.system(.body, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .truncationMode(.middle)
                            .multilineTextAlignment(.trailing)
                    }

                    HStack {
                        Spacer()
                        Button("Reveal in Finder") {
                            NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: workspace.rootPath)
                        }
                        Button("Change Workspace…") {
                            appModel.openWorkspace()
                        }
                        Button("Close Workspace", role: .destructive) {
                            appModel.closeWorkspace()
                        }
                    }
                } else {
                    Label("No workspace is open", systemImage: "folder.badge.questionmark")
                        .foregroundStyle(.secondary)

                    Button("Open Workspace…") {
                        appModel.openWorkspace()
                    }
                    .buttonStyle(.borderedProminent)
                }
            } header: {
                SettingsSectionHeader(
                    title: "Current Workspace",
                    message: "A workspace contains your local study data and imported sources."
                )
            }

            Section {
                Text("StudyPulse remembers the last workspace you opened and restores its access when possible.")
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            } header: {
                Text("Storage")
            }
        }
        .formStyle(.grouped)
    }
}

private struct AboutSettingsView: View {
    private var version: String {
        let info = Bundle.main.infoDictionary
        let marketing = info?["CFBundleShortVersionString"] as? String ?? "1.0"
        let build = info?["CFBundleVersion"] as? String ?? "1"
        return "\(marketing) (\(build))"
    }

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "waveform.path.ecg")
                .font(.system(size: 48, weight: .medium))
                .foregroundStyle(.tint)

            VStack(spacing: 4) {
                Text("StudyPulse")
                    .font(.title2.weight(.semibold))
                Text("A calm workspace for deliberate learning.")
                    .foregroundStyle(.secondary)
            }

            Text(L10n.format("Version %@", version))
                .font(.caption)
                .foregroundStyle(.tertiary)

            Divider()

            Text("Your workspaces stay local on this Mac. Cloud AI is an optional connected service for Agent features.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 400)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }
}

private struct SettingsSectionHeader: View {
    let title: String
    let message: String

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(LocalizedStringKey(title))
            Text(LocalizedStringKey(message))
                .font(.caption)
                .foregroundStyle(.secondary)
                .textCase(nil)
        }
    }
}

private struct SettingsShortcutRow: View {
    let title: String
    let shortcut: String

    var body: some View {
        LabeledContent(LocalizedStringKey(title)) {
            Text(shortcut)
                .font(.system(.body, design: .rounded).weight(.medium))
                .foregroundStyle(.secondary)
        }
    }
}
