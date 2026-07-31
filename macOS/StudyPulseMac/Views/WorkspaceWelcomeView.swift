import SwiftUI

struct WorkspaceWelcomeView: View {
    @ObservedObject var appModel: AppViewModel

    var body: some View {
        VStack(spacing: 24) {
            Image(systemName: "waveform.path.ecg")
                .font(.system(size: 56, weight: .medium))
                .foregroundStyle(.tint)
                .accessibilityHidden(true)

            VStack(spacing: 8) {
                Text("StudyPulse")
                    .font(.largeTitle.weight(.semibold))
                Text("Create or open a Workspace to start a local learning Agent.")
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            HStack(spacing: 12) {
                Button("Open Workspace…") {
                    appModel.openWorkspace()
                }
                .controlSize(.large)

                Button("Create Workspace…") {
                    appModel.createWorkspace()
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
            }

            if appModel.workspace.isBusy {
                ProgressView("Opening Workspace…")
                    .controlSize(.small)
            }
        }
        .frame(maxWidth: 520)
        .padding(48)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
