import Foundation
import UIKit

/// Where Syrup's brain is, and how to reach it. The app (the mouth) and its
/// screen broadcast (the eyes) both use this. The server is set when the app
/// is built: `SYRUP_SERVER_URL`, and `SYRUP_TOKEN` for a hosted server.
enum Link {
    static let server: URL? = {
        guard let raw = Bundle.main.object(forInfoDictionaryKey: "SyrupServerURL") as? String else { return nil }
        let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !s.isEmpty, !s.hasPrefix("$("), let url = URL(string: s), url.scheme != nil, url.host != nil else {
            return nil
        }
        return url
    }()

    static let token: String? = {
        guard let raw = Bundle.main.object(forInfoDictionaryKey: "SyrupToken") as? String else { return nil }
        let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        return s.isEmpty || s.hasPrefix("$(") ? nil : s
    }()

    /// This phone, the same for the app and for its broadcast.
    static var device: String {
        UIDevice.current.identifierForVendor?.uuidString ?? "unknown-phone"
    }

    static func request(
        _ path: String,
        _ query: [String: String] = [:],
        method: String = "GET",
        body: Data? = nil,
        contentType: String? = nil,
        timeout: TimeInterval = 15
    ) -> URLRequest? {
        guard let server, var parts = URLComponents(url: server, resolvingAgainstBaseURL: false) else { return nil }
        let base = parts.path.hasSuffix("/") ? String(parts.path.dropLast()) : parts.path
        parts.path = base + path
        var items = [URLQueryItem(name: "device", value: device)]
        for key in query.keys.sorted() {
            items.append(URLQueryItem(name: key, value: query[key]))
        }
        parts.queryItems = items
        guard let url = parts.url else { return nil }
        var r = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: timeout)
        r.httpMethod = method
        r.httpBody = body
        if let contentType {
            r.setValue(contentType, forHTTPHeaderField: "Content-Type")
        }
        if let token {
            r.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        return r
    }

    /// Why the brain said no, for people.
    static func explain(_ status: Int, _ data: Data?) -> String {
        if let data,
           let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let message = object["error"] as? String
        {
            return message
        }
        switch status {
        case 401: return "it wants a different token"
        case 403: return "it belongs to another phone"
        default: return "it answered \(status)"
        }
    }
}

/// One thing Syrup said.
struct SaidLine: Decodable {
    let seq: UInt64
    let text: String
    let end: Bool
}

/// What `GET /v1/say` answers.
struct SayAnswer: Decodable {
    let lines: [SaidLine]
    let last: UInt64
    let watching: Bool
}

/// What `POST /v1/frame` answers.
struct FrameAnswer: Decodable {
    struct Next: Decodable {
        let interval_ms: Int
        let max_side: Int
        let quality: Double
    }

    let say: [String]
    let mouth: Bool
    let next: Next
}
