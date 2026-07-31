import AppKit
import AuthenticationServices
import Foundation
import Security

@MainActor
protocol CloudAuthPresenting: AnyObject {
    func authenticate(loginURL: String) async throws -> String
}

enum CloudPlatformAuthError: Error, LocalizedError, Sendable {
    case invalidLoginURL
    case cancelled
    case callbackMissing
    case presentationUnavailable
    case authenticationFailed(String)
    case keychain(OSStatus)

    var errorDescription: String? {
        switch self {
        case .invalidLoginURL:
            L10n.string("Cloud AI returned an invalid login URL.")
        case .cancelled:
            L10n.string("Cloud AI sign-in was cancelled.")
        case .callbackMissing:
            L10n.string("Cloud AI did not return a login callback.")
        case .presentationUnavailable:
            L10n.string("A window is required to present Cloud AI sign-in.")
        case .authenticationFailed(let message):
            L10n.format("Cloud AI sign-in failed: %@", message)
        case .keychain(let status):
            L10n.format("Cloud AI credentials could not be stored in Keychain (%@).", String(status))
        }
    }
}

@MainActor
final class CloudWebAuthPresenter: NSObject, CloudAuthPresenting,
    ASWebAuthenticationPresentationContextProviding
{
    private var session: ASWebAuthenticationSession?

    func authenticate(loginURL: String) async throws -> String {
        guard let url = URL(string: loginURL) else {
            throw CloudPlatformAuthError.invalidLoginURL
        }
        defer { session = nil }
        return try await withCheckedThrowingContinuation { continuation in
            let bridge = CloudWebAuthContinuation(continuation)
            let session = ASWebAuthenticationSession(
                url: url,
                callbackURLScheme: "studypulse",
                completionHandler: Self.completionHandler(for: bridge)
            )
            session.presentationContextProvider = self
            session.prefersEphemeralWebBrowserSession = false
            self.session = session
            if !session.start() {
                bridge.resume(.failure(.presentationUnavailable))
            }
        }
    }

    nonisolated static func completionHandler(
        for bridge: CloudWebAuthContinuation
    ) -> ASWebAuthenticationSession.CompletionHandler {
        { callbackURL, error in
            bridge.resume(callbackResult(callbackURL: callbackURL, error: error))
        }
    }

    nonisolated static func callbackResult(
        callbackURL: URL?,
        error: (any Error)?
    ) -> Result<String, CloudPlatformAuthError> {
        if let authenticationError = error as? ASWebAuthenticationSessionError,
           authenticationError.code == .canceledLogin
        {
            return .failure(.cancelled)
        }
        if let error {
            return .failure(.authenticationFailed(error.localizedDescription))
        }
        if let callbackURL {
            return .success(callbackURL.absoluteString)
        }
        return .failure(.callbackMissing)
    }

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        NSApplication.shared.keyWindow
            ?? NSApplication.shared.windows.first(where: \.isVisible)
            ?? ASPresentationAnchor()
    }
}

final class CloudWebAuthContinuation: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<String, any Error>?

    init(_ continuation: CheckedContinuation<String, any Error>) {
        self.continuation = continuation
    }

    func resume(_ result: Result<String, CloudPlatformAuthError>) {
        lock.lock()
        guard let continuation else {
            lock.unlock()
            return
        }
        self.continuation = nil
        lock.unlock()

        switch result {
        case .success(let callback):
            continuation.resume(returning: callback)
        case .failure(let error):
            continuation.resume(throwing: error)
        }
    }
}

struct StoredCloudTokens: Codable, Equatable, Sendable {
    let accessToken: String
    let refreshToken: String
}

protocol CloudCredentialStoring: Sendable {
    func load() throws -> StoredCloudTokens?
    func save(_ tokens: StoredCloudTokens) throws
    func clear() throws
}

struct CloudCredentialStore: CloudCredentialStoring {
    private let service = "space.chenkai.StudyPulse-Desktop.CloudAI"
    private let account = "session-token-pair"

    func load() throws -> StoredCloudTokens? {
        var query = baseQuery
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound {
            return nil
        }
        guard status == errSecSuccess, let data = result as? Data else {
            throw CloudPlatformAuthError.keychain(status)
        }
        return try JSONDecoder().decode(StoredCloudTokens.self, from: data)
    }

    func save(_ tokens: StoredCloudTokens) throws {
        let data = try JSONEncoder().encode(tokens)
        let status = SecItemUpdate(
            baseQuery as CFDictionary,
            [kSecValueData as String: data] as CFDictionary
        )
        if status == errSecItemNotFound {
            var item = baseQuery
            item[kSecValueData as String] = data
            item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let addStatus = SecItemAdd(item as CFDictionary, nil)
            guard addStatus == errSecSuccess else {
                throw CloudPlatformAuthError.keychain(addStatus)
            }
        } else if status != errSecSuccess {
            throw CloudPlatformAuthError.keychain(status)
        }
    }

    func clear() throws {
        let status = SecItemDelete(baseQuery as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw CloudPlatformAuthError.keychain(status)
        }
    }

    private var baseQuery: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
        ]
    }
}

struct StoredBYOKConfig: Codable, Equatable, Sendable {
    let apiKey: String
    let baseURL: String
    let model: String
}

protocol BYOKCredentialStoring: Sendable {
    func load() throws -> StoredBYOKConfig?
    func save(_ config: StoredBYOKConfig) throws
    func clear() throws
}

struct BYOKCredentialStore: BYOKCredentialStoring {
    private let service = "space.chenkai.StudyPulse-Desktop.BYOK"
    private let account = "openai-compatible"

    func load() throws -> StoredBYOKConfig? {
        var query = baseQuery
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound {
            return nil
        }
        guard status == errSecSuccess, let data = result as? Data else {
            throw CloudPlatformAuthError.keychain(status)
        }
        return try JSONDecoder().decode(StoredBYOKConfig.self, from: data)
    }

    func save(_ config: StoredBYOKConfig) throws {
        let data = try JSONEncoder().encode(config)
        let status = SecItemUpdate(
            baseQuery as CFDictionary,
            [kSecValueData as String: data] as CFDictionary
        )
        if status == errSecItemNotFound {
            var item = baseQuery
            item[kSecValueData as String] = data
            item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            let addStatus = SecItemAdd(item as CFDictionary, nil)
            guard addStatus == errSecSuccess else {
                throw CloudPlatformAuthError.keychain(addStatus)
            }
        } else if status != errSecSuccess {
            throw CloudPlatformAuthError.keychain(status)
        }
    }

    func clear() throws {
        let status = SecItemDelete(baseQuery as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw CloudPlatformAuthError.keychain(status)
        }
    }

    private var baseQuery: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
        ]
    }
}
