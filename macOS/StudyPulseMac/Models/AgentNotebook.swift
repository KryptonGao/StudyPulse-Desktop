import Foundation

enum AgentChatRole: String, Codable, Equatable {
    case user
    case assistant
}

struct AgentChatMessage: Codable, Equatable, Identifiable {
    let id: UUID
    let role: AgentChatRole
    var content: String
    let createdAt: Date

    init(
        id: UUID = UUID(),
        role: AgentChatRole,
        content: String,
        createdAt: Date = .now
    ) {
        self.id = id
        self.role = role
        self.content = content
        self.createdAt = createdAt
    }
}

struct AgentNotebook: Codable, Equatable, Identifiable {
    let id: UUID
    var title: String
    var sourcePaths: [String]
    var messages: [AgentChatMessage]
    var lastGoal: String
    var lastAnswer: String
    var updatedAt: Date

    init(
        id: UUID = UUID(),
        title: String,
        sourcePaths: [String] = [],
        messages: [AgentChatMessage] = [],
        lastGoal: String = "",
        lastAnswer: String = "",
        updatedAt: Date = .now
    ) {
        self.id = id
        self.title = title
        self.sourcePaths = sourcePaths
        self.messages = messages
        self.lastGoal = lastGoal
        self.lastAnswer = lastAnswer
        self.updatedAt = updatedAt
    }
}

extension AgentNotebook {
    init(dto: AgentNotebookDto) {
        let updatedAt = ISO8601DateFormatter().date(from: dto.updatedAt) ?? .now
        var messages = dto.messages.map(AgentChatMessage.init(dto:))
        if messages.isEmpty {
            if !dto.lastGoal.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                messages.append(AgentChatMessage(
                    role: .user,
                    content: dto.lastGoal,
                    createdAt: updatedAt.addingTimeInterval(-1)
                ))
            }
            if !dto.lastAnswer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                messages.append(AgentChatMessage(
                    role: .assistant,
                    content: dto.lastAnswer,
                    createdAt: updatedAt
                ))
            }
        }
        self.init(
            id: UUID(uuidString: dto.id) ?? UUID(),
            title: dto.title,
            sourcePaths: dto.sourcePaths,
            messages: messages,
            lastGoal: dto.lastGoal,
            lastAnswer: dto.lastAnswer,
            updatedAt: updatedAt
        )
    }

    var dto: AgentNotebookDto {
        let latestGoal = messages.last(where: { $0.role == .user })?.content ?? lastGoal
        let latestAnswer = messages.last(where: { $0.role == .assistant })?.content ?? lastAnswer
        return AgentNotebookDto(
            id: id.uuidString,
            title: title,
            sourcePaths: sourcePaths,
            messages: messages.map(\.dto),
            lastGoal: latestGoal,
            lastAnswer: latestAnswer,
            updatedAt: ISO8601DateFormatter().string(from: updatedAt)
        )
    }
}

extension AgentChatMessage {
    init(dto: AgentMessageDto) {
        self.init(
            id: UUID(uuidString: dto.id) ?? UUID(),
            role: dto.role == .user ? .user : .assistant,
            content: dto.content,
            createdAt: ISO8601DateFormatter().date(from: dto.createdAt) ?? .now
        )
    }

    var dto: AgentMessageDto {
        AgentMessageDto(
            id: id.uuidString,
            role: role == .user ? .user : .assistant,
            content: content,
            createdAt: ISO8601DateFormatter().string(from: createdAt)
        )
    }
}
