import AuthenticationServices
import CryptoKit
import Foundation
import Security
import UIKit

struct Position: Codable {
    var latitude: Double
    var longitude: Double
}

struct Circle: Codable {
    var center: Position
    var radiusMeters: Double
}

struct EventArea: Codable {
    var area: Circle
    var expiresAt: Date
}

struct Hours: Codable {
    var weekdays: [Int]
    var opens: String
    var closes: String
}

struct Settings: Codable {
    var name = ""
    var cuisine = ""
    var timezone = TimeZone.current.identifier
    var hours = Hours(weekdays: [2, 3, 4, 5, 6], opens: "11:00:00", closes: "15:00:00")
    var area = Circle(center: Position(latitude: 34.540, longitude: -112.468), radiusMeters: 8_000)
    var event: EventArea?
}

struct Menu: Codable {
    let id: UUID
    let url: URL
    let capturedAt: Date
    let publishedAt: Date
}

struct MenuItem: Codable, Identifiable {
    var id = UUID()
    var position = 0
    var name = ""
    var description: String?
    var priceText: String?
    var tagIds: [String] = []
}

struct MenuTag: Decodable, Identifiable {
    let id: String
    let label: String
}

struct MenuExtraction: Decodable {
    let menuId: UUID
    let revision: UUID
    let status: String
    let items: [MenuItem]
}

struct MenuResults: Decodable {
    let extraction: MenuExtraction?
    let tags: [MenuTag]
}

struct MenuCorrection: Encodable {
    let menuId: UUID
    let revision: UUID?
    let items: [MenuItem]
}

struct Profile: Codable {
    let id: UUID
    let settings: Settings
    let menu: Menu?
}

enum TruckStatus: String, Codable {
    case pending, approved, rejected
}

struct Registration: Codable {
    let name: String
    let cuisine: String
    let status: TruckStatus
}

struct Fix: Codable {
    let position: Position
    let accuracyMeters: Double
    let recordedAt: Date
}

struct Sharing: Codable {
    let id: UUID
    let publishedAt: Date
    let expiresAt: Date
    let closesAt: Date
}

struct OperatorView: Codable {
    let profile: Profile?
    let session: Sharing?
    let registration: Registration?
}

struct LoginConfig: Codable {
    let issuer: URL
    let audience: String
    let connection: String
    let iosClientId: String
    let redirectUri: String
}

struct UploadTicket: Codable {
    let id: UUID
    let url: URL
}

struct ServerError: LocalizedError {
    let code: Int
    let message: String
    var errorDescription: String? { message }
}

private struct Failure: Decodable { let error: String }

private func decoder() -> JSONDecoder {
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .custom { source in
        let value = try source.singleValueContainer().decode(String.self)
        let precise = ISO8601DateFormatter()
        precise.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = precise.date(from: value) ?? ISO8601DateFormatter().date(from: value) { return date }
        throw DecodingError.dataCorruptedError(in: try source.singleValueContainer(), debugDescription: "Expected an ISO-8601 date")
    }
    return decoder
}

private func checkedData(_ data: Data, _ response: URLResponse) throws -> Data {
    guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        let message = (try? decoder().decode(Failure.self, from: data))?.error
        throw ServerError(code: code, message: message ?? "The request could not be completed.")
    }
    return data
}

struct API {
    private let base: URL

    init() {
        guard let origin = Bundle.main.object(forInfoDictionaryKey: "RollingLocalAPIOrigin") as? String,
              let url = URL(string: origin), url.scheme == "https", url.host != nil else {
            preconditionFailure("Set RollingLocalAPIOrigin to an HTTPS API origin.")
        }
        base = url
    }

    func request<T: Decodable>(_ path: String, method: String = "GET", bearer: String? = nil, body: (any Encodable)? = nil) async throws -> T {
        var request = URLRequest(url: base.appendingPathComponent(path))
        request.httpMethod = method
        request.timeoutInterval = 15
        if let bearer { request.setValue("Bearer \(bearer)", forHTTPHeaderField: "Authorization") }
        if let body {
            let encoder = JSONEncoder()
            encoder.dateEncodingStrategy = .iso8601
            request.httpBody = try encoder.encode(body)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await URLSession.shared.data(for: request)
        return try decoder().decode(T.self, from: checkedData(data, response))
    }

    func upload(_ jpeg: Data, to url: URL) async throws {
        guard url.scheme == "https" else { throw ServerError(code: 0, message: "Invalid upload URL.") }
        var request = URLRequest(url: url)
        request.httpMethod = "PUT"
        request.timeoutInterval = 45
        request.setValue("image/jpeg", forHTTPHeaderField: "Content-Type")
        let (data, response) = try await URLSession.shared.upload(for: request, from: jpeg)
        _ = try checkedData(data, response)
    }
}

private struct Tokens: Codable {
    let accessToken: String
    let refreshToken: String?
    let expiresIn: Int

    enum CodingKeys: String, CodingKey {
        case accessToken = "access_token"
        case refreshToken = "refresh_token"
        case expiresIn = "expires_in"
    }
}

@MainActor final class Login: NSObject, ASWebAuthenticationPresentationContextProviding {
    private let vault: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: "RollingLocal",
        kSecAttrAccount as String: "auth0-refresh"
    ]
    private var access: String?
    private var expiresAt = Date.distantPast
    private var webSession: ASWebAuthenticationSession?
    private var refreshTask: Task<String, Error>?

    var canRestore: Bool { refreshToken != nil }

    private var refreshToken: String? {
        var query = vault
        query[kSecReturnData as String] = true
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    func clear() {
        refreshTask?.cancel()
        refreshTask = nil
        access = nil
        expiresAt = .distantPast
        SecItemDelete(vault as CFDictionary)
    }

    func signIn(config: LoginConfig) async throws {
        var bytes = [UInt8](repeating: 0, count: 32)
        guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess,
              let callback = URL(string: config.redirectUri), let scheme = callback.scheme else {
            throw ServerError(code: 0, message: "The sign-in configuration is invalid.")
        }
        let verifier = urlSafe(Data(bytes))
        let challenge = urlSafe(Data(SHA256.hash(data: Data(verifier.utf8))))
        let state = UUID().uuidString
        var url = URLComponents(url: config.issuer.appendingPathComponent("authorize"), resolvingAgainstBaseURL: false)!
        url.queryItems = [
            URLQueryItem(name: "response_type", value: "code"),
            URLQueryItem(name: "client_id", value: config.iosClientId),
            URLQueryItem(name: "redirect_uri", value: config.redirectUri),
            URLQueryItem(name: "scope", value: "openid email offline_access"),
            URLQueryItem(name: "audience", value: config.audience),
            URLQueryItem(name: "connection", value: config.connection),
            URLQueryItem(name: "state", value: state),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
            URLQueryItem(name: "code_challenge", value: challenge)
        ]
        let result: URL = try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(url: url.url!, callbackURLScheme: scheme) { callback, error in
                if let callback { continuation.resume(returning: callback) }
                else { continuation.resume(throwing: error ?? ServerError(code: 0, message: "Sign-in was cancelled.")) }
            }
            session.presentationContextProvider = self
            webSession = session
            if !session.start() {
                continuation.resume(throwing: ServerError(code: 0, message: "Could not open sign-in."))
            }
        }
        webSession = nil
        let items = URLComponents(url: result, resolvingAgainstBaseURL: false)?.queryItems ?? []
        guard result.scheme == callback.scheme, result.host == callback.host, result.path == callback.path,
              items.first(where: { $0.name == "state" })?.value == state,
              let code = items.first(where: { $0.name == "code" })?.value else {
            throw ServerError(code: 401, message: "Ahara sign-in did not authorize this account.")
        }
        _ = try await exchange(config: config, parameters: [
            "grant_type": "authorization_code", "code": code,
            "redirect_uri": config.redirectUri, "code_verifier": verifier
        ])
    }

    func token(config: LoginConfig) async throws -> String {
        if let access, expiresAt.timeIntervalSinceNow > 45 { return access }
        if let refreshTask { return try await refreshTask.value }
        guard let refreshToken else { throw ServerError(code: 401, message: "Sign in to continue.") }
        let task = Task {
            try await exchange(config: config, parameters: ["grant_type": "refresh_token", "refresh_token": refreshToken])
        }
        refreshTask = task
        defer { refreshTask = nil }
        return try await task.value
    }

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows).first(where: \.isKeyWindow) ?? ASPresentationAnchor()
    }

    private func exchange(config: LoginConfig, parameters: [String: String]) async throws -> String {
        var request = URLRequest(url: config.issuer.appendingPathComponent("oauth/token"))
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        var values = parameters
        values["client_id"] = config.iosClientId
        let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
        let parts = values.map { key, value in "\(key)=\(value.addingPercentEncoding(withAllowedCharacters: allowed) ?? "")" }
        request.httpBody = Data(parts.joined(separator: "&").utf8)
        let (data, response) = try await URLSession.shared.data(for: request)
        let tokens = try decoder().decode(Tokens.self, from: checkedData(data, response))
        try Task.checkCancellation()
        if let refresh = tokens.refreshToken {
            var item = vault
            item[kSecValueData as String] = Data(refresh.utf8)
            item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            SecItemDelete(vault as CFDictionary)
            guard SecItemAdd(item as CFDictionary, nil) == errSecSuccess else {
                throw ServerError(code: 0, message: "Could not save your sign-in on this device.")
            }
        }
        access = tokens.accessToken
        expiresAt = .now.addingTimeInterval(TimeInterval(tokens.expiresIn))
        return tokens.accessToken
    }

    private func urlSafe(_ data: Data) -> String {
        data.base64EncodedString().replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: "=", with: "")
    }
}
