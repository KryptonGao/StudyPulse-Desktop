import SwiftUI

struct BackupImportSheet: View {
    @ObservedObject var appModel: AppViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Import iOS Backup")
                .font(.title2.weight(.semibold))

            if appModel.backup.isBusy {
                ProgressView("Validating backup…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let inspection = appModel.backup.inspection {
                inspectionView(inspection)
            } else if let error = appModel.backup.errorMessage {
                ContentUnavailableView(
                    "Backup could not be inspected",
                    systemImage: "exclamationmark.triangle",
                    description: Text(error)
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .padding(24)
        .frame(minWidth: 620, minHeight: 480)
        .interactiveDismissDisabled(appModel.backup.isBusy)
        .onDisappear {
            if appModel.backup.inspection != nil {
                appModel.backup.cancel()
            }
        }
    }

    private func inspectionView(_ inspection: BackupInspectionDto) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 24) {
                LabeledContent("Schema", value: "\(inspection.schemaVersion)")
                LabeledContent("New", value: "\(inspection.addedRecords)")
                LabeledContent("Identical", value: "\(inspection.identicalRecords)")
                LabeledContent("Conflicts", value: "\(inspection.conflicts.count)")
            }

            Picker(
                "Import mode",
                selection: Binding(
                    get: { appModel.backup.mode },
                    set: { appModel.backup.mode = $0 }
                )
            ) {
                Text("Merge").tag(RestoreModeDto.merge)
                Text("Replace").tag(RestoreModeDto.replace)
            }
            .pickerStyle(.segmented)

            if inspection.conflicts.isEmpty {
                ContentUnavailableView(
                    "No conflicts",
                    systemImage: "checkmark.circle",
                    description: Text("The backup is ready to apply.")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(inspection.conflicts, id: \.key) { conflict in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(conflict.displayName)
                                .font(.headline)
                            Text(conflict.domain)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Picker(
                            "Resolution",
                            selection: Binding(
                                get: { appModel.backup.choice(for: conflict.key) },
                                set: { appModel.backup.setChoice($0, for: conflict.key) }
                            )
                        ) {
                            Text("Use imported").tag(true)
                            Text("Keep local").tag(false)
                        }
                        .labelsHidden()
                        .frame(width: 150)
                    }
                }
            }

            if !inspection.warnings.isEmpty {
                GroupBox("Warnings") {
                    ForEach(inspection.warnings, id: \.self) { warning in
                        Label(warning, systemImage: "exclamationmark.triangle")
                            .font(.caption)
                    }
                }
            }

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) {
                    appModel.backup.cancel()
                    appModel.isShowingBackupImport = false
                }
                Button("Apply Import") {
                    Task {
                        if await appModel.backup.apply() {
                            await appModel.didApplyBackup()
                        }
                    }
                }
                .buttonStyle(.borderedProminent)
            }
        }
    }
}
