import Combine
import Foundation
import SwiftStreamingMarkdown

@MainActor
final class AgentMarkdownSource: ObservableObject, StreamedMarkdownSource {
    nonisolated let text: AsyncStream<String>
    private nonisolated let continuation: AsyncStream<String>.Continuation
    private var currentText: String

    init(text: String) {
        let stream = AsyncStream.makeStream(
            of: String.self,
            bufferingPolicy: .bufferingNewest(1)
        )
        self.text = stream.stream
        continuation = stream.continuation
        currentText = text
        continuation.yield(text)
    }

    func update(text: String) {
        guard text != currentText else { return }
        currentText = text
        continuation.yield(text)
    }

    deinit {
        continuation.finish()
    }
}
