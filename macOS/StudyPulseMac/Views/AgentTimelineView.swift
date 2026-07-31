import SwiftUI

struct AgentTimelineView: View {
    @ObservedObject var viewModel: AgentViewModel

    var body: some View {
        if viewModel.events.isEmpty {
            ContentUnavailableView(
                "No activity yet",
                systemImage: "timeline.selection",
                description: Text("Agent events will appear here in order.")
            )
        } else {
            List(viewModel.events) { event in
                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: event.kind.symbol)
                        .foregroundStyle(event.kind.tint)
                        .frame(width: 20)

                    VStack(alignment: .leading, spacing: 4) {
                        Text(event.kind.displayName)
                            .font(.subheadline.weight(.semibold))
                        if let tool = event.toolName {
                            Text(event.toolDisplayName ?? tool)
                                .font(.caption.weight(.medium))
                                .foregroundStyle(.primary)
                        }
                        if let permission = event.permission {
                            Label(permission.displayName, systemImage: "lock.shield")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        if let stage = event.stage, !stage.isEmpty {
                            HStack(spacing: 6) {
                                Text(stage.capitalized)
                                    .font(.caption.weight(.medium))
                                if let progress = event.progress {
                                    ProgressView(value: progress)
                                        .frame(width: 90)
                                }
                            }
                            .foregroundStyle(.tint)
                        }
                        if let text = event.timelineDetail, !text.isEmpty {
                            Text(text)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(4)
                        }
                        Text("#\(event.sequence)")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }
                }
                .padding(.vertical, 4)
            }
        }
    }
}

private extension AgentEventDto {
    var timelineDetail: String? {
        preview ?? text ?? payloadJson
    }
}
