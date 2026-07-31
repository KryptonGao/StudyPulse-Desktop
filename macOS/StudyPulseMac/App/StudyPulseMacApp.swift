import SwiftUI

@main
struct StudyPulseMacApp: App {
    @StateObject private var appModel = AppViewModel()
    @AppStorage("StudyPulse.appearance") private var appearanceRawValue = StudyPulseAppearance.system.rawValue
    @AppStorage("StudyPulse.language") private var languageRawValue = AppLanguage.system.rawValue

    var body: some Scene {
        WindowGroup {
            RootView(appModel: appModel)
                .frame(minWidth: 980, minHeight: 640)
                .preferredColorScheme(appearance.colorScheme)
                .environment(\.locale, language.locale)
        }
        .defaultSize(width: 1280, height: 780)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Workspace…") {
                    appModel.createWorkspace()
                }
                .keyboardShortcut("n", modifiers: [.command, .shift])

                Button("Open Workspace…") {
                    appModel.openWorkspace()
                }
                .keyboardShortcut("o", modifiers: .command)
            }
        }

        Settings {
            SettingsView(appModel: appModel)
                .preferredColorScheme(appearance.colorScheme)
                .environment(\.locale, language.locale)
        }
        .defaultSize(width: 640, height: 520)
    }

    private var appearance: StudyPulseAppearance {
        StudyPulseAppearance(rawValue: appearanceRawValue) ?? .system
    }

    private var language: AppLanguage {
        AppLanguage(rawValue: languageRawValue) ?? .system
    }
}
