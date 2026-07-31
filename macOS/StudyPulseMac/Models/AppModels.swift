import Foundation
import SwiftUI

enum AgentProvider: String {
    case cloud
    case byok
}

enum AppDestination: String, CaseIterable, Identifiable {
    case today
    case agent
    case subjects
    case exams
    case mistakes
    case tasks
    case timer
    case timeInvestment
    case library

    var id: String { rawValue }

    var title: String {
        switch self {
        case .today: L10n.string("Today")
        case .agent: L10n.string("Agent")
        case .subjects: L10n.string("Subjects & Grades")
        case .exams: L10n.string("Exams")
        case .mistakes: L10n.string("Mistakes")
        case .tasks: L10n.string("Tasks")
        case .timer: L10n.string("Study Timer")
        case .timeInvestment: L10n.string("Time Investment")
        case .library: L10n.string("Library")
        }
    }

    var symbol: String {
        switch self {
        case .today: "sun.max"
        case .agent: "sparkles"
        case .subjects: "book.closed"
        case .exams: "calendar.badge.clock"
        case .mistakes: "exclamationmark.bubble"
        case .tasks: "checklist"
        case .timer: "timer"
        case .timeInvestment: "chart.bar.xaxis"
        case .library: "books.vertical"
        }
    }
}

struct PendingConfirmation: Identifiable {
    let id: String
    let runID: String
    let toolName: String
    let preview: String
    let permission: PermissionDto
    let payloadJSON: String
}

struct PendingAgentInput: Identifiable {
    let id: String
    let runID: String
    let prompt: String
    let options: [String]
}

extension AgentModeDto: CaseIterable, Identifiable {
    public var id: String { String(describing: self) }

    public static var allCases: [AgentModeDto] {
        [.chat, .deepSolve, .mastery, .deepResearch, .questionLab, .visualize]
    }

    var title: String {
        switch self {
        case .chat: L10n.string("Chat")
        case .deepSolve: L10n.string("Deep Solve")
        case .mastery: L10n.string("Mastery")
        case .deepResearch: L10n.string("Deep Research")
        case .questionLab: L10n.string("Question Lab")
        case .visualize: L10n.string("Visualize")
        }
    }

    var symbol: String {
        switch self {
        case .chat: "bubble.left.and.bubble.right"
        case .deepSolve: "function"
        case .mastery: "graduationcap"
        case .deepResearch: "magnifyingglass"
        case .questionLab: "list.number"
        case .visualize: "chart.xyaxis.line"
        }
    }
}

struct TaskSection: Identifiable {
    let id: String
    let title: String
    let tasks: [TaskDto]
}

enum TaskFilter: String, CaseIterable, Identifiable {
    case all
    case homework
    case reading

    var id: String { rawValue }

    var title: String {
        switch self {
        case .all: L10n.string("All")
        case .homework: L10n.string("Homework")
        case .reading: L10n.string("Reading")
        }
    }
}

extension TaskDto: Identifiable {}

extension AgentEventDto: Identifiable {
    public var id: String { "\(runId)-\(sequence)" }
}

extension RunStatusDto {
    var displayName: String {
        switch self {
        case .started: L10n.string("Started")
        case .running: L10n.string("Running")
        case .waitingForConfirmation: L10n.string("Waiting for confirmation")
        case .cancelling: L10n.string("Cancelling")
        case .completed: L10n.string("Completed")
        case .failed: L10n.string("Failed")
        case .cancelled: L10n.string("Cancelled")
        }
    }
}

extension PermissionDto {
    var displayName: String {
        switch self {
        case .read: L10n.string("Read")
        case .write: L10n.string("Write")
        case .destructive: L10n.string("Destructive")
        case .execute: L10n.string("Execute")
        }
    }
}

extension AgentEventDto {
    var toolDisplayName: String? {
        guard let toolName else { return nil }
        return switch toolName {
        case "list_workspace_files": L10n.string("Browse notebook files")
        case "search_workspace": L10n.string("Search notebook")
        case "read_source": L10n.string("Read source")
        case "read_memory": L10n.string("Read Agent memory")
        case "write_memory": L10n.string("Update Agent memory")
        case "web_search": L10n.string("Search the web")
        case "paper_search": L10n.string("Search papers")
        case "code_execution": L10n.string("Run Python locally")
        case "save_artifact": L10n.string("Save artifact")
        case "ask_user": L10n.string("Ask you a question")
        case "get_tasks": L10n.string("Read tasks")
        case "create_task": L10n.string("Create task")
        default: toolName.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }
}

extension TaskTypeDto {
    var displayName: String {
        switch self {
        case .homework: L10n.string("Homework")
        case .reading: L10n.string("Reading")
        }
    }
}

extension TimeInvestmentThemeDto {
    var displayName: String {
        switch self {
        case .ocean: L10n.string("Ocean")
        case .coral: L10n.string("Coral")
        case .violet: L10n.string("Violet")
        case .sunshine: L10n.string("Sunshine")
        case .mint: L10n.string("Mint")
        }
    }
}

extension AgentEventKindDto {
    var displayName: String {
        switch self {
        case .started: L10n.string("Started")
        case .statusChanged: L10n.string("Status changed")
        case .textDelta: L10n.string("Response")
        case .toolRequested: L10n.string("Tool requested")
        case .toolCompleted: L10n.string("Tool completed")
        case .confirmationRequired: L10n.string("Confirmation required")
        case .stageStarted: L10n.string("Stage started")
        case .stageProgress: L10n.string("Stage progress")
        case .stageCompleted: L10n.string("Stage completed")
        case .inputRequired: L10n.string("Input required")
        case .artifactCreated: L10n.string("Artifact created")
        case .failed: L10n.string("Failed")
        case .cancelled: L10n.string("Cancelled")
        case .completed: L10n.string("Completed")
        }
    }

    var symbol: String {
        switch self {
        case .started: "play.circle"
        case .statusChanged: "arrow.triangle.2.circlepath"
        case .textDelta: "text.bubble"
        case .toolRequested: "wrench.and.screwdriver"
        case .toolCompleted: "checkmark.circle"
        case .confirmationRequired: "hand.raised"
        case .stageStarted: "play.circle"
        case .stageProgress: "chart.bar"
        case .stageCompleted: "checkmark.circle"
        case .inputRequired: "questionmark.bubble"
        case .artifactCreated: "paperclip"
        case .failed: "exclamationmark.octagon"
        case .cancelled: "xmark.circle"
        case .completed: "checkmark.seal"
        }
    }

    var tint: Color {
        switch self {
        case .failed: .red
        case .confirmationRequired: .orange
        case .completed, .toolCompleted: .green
        case .cancelled: .secondary
        default: .accentColor
        }
    }
}
