import AVFoundation
import SwiftUI
import UIKit
import UserNotifications

/// The half of the app that talks. It keeps asking Syrup's brain what to say,
/// and says it. An audio session stays open (a silent sound on a loop, mixed
/// with the game's) so it can do that from the background while a game is in
/// front, the way a voice assistant can. While Syrup speaks, the game's sound
/// is turned down.
@MainActor
final class Mouth: NSObject, ObservableObject {
    @Published private(set) var status = "Starting…"
    @Published private(set) var lastLine: String?
    @Published private(set) var watching = false
    /// Whether the brain answered last time (nil: not asked yet).
    @Published private(set) var reachable: Bool?

    private let voice = AVSpeechSynthesizer()
    private var hum: AVAudioPlayer?
    private var loop: Task<Void, Never>?
    private var quietSince: Date?
    private let session = URLSession(configuration: .default)

    override init() {
        super.init()
        voice.delegate = self
        NotificationCenter.default.addObserver(
            forName: AVAudioSession.interruptionNotification, object: nil, queue: .main
        ) { [weak self] note in
            guard let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
                  AVAudioSession.InterruptionType(rawValue: raw) == .ended
            else { return }
            Task { @MainActor in self?.resume() }
        }
    }

    /// Starts listening (nothing happens if it already is).
    func start() {
        guard loop == nil else { return }
        guard Link.server != nil else {
            status = "This build has no server. Add SYRUP_SERVER_URL to the repository's secrets, and build again."
            return
        }
        stayAwake()
        status = "Getting ready…"
        loop = Task { [weak self] in
            await self?.listen()
        }
    }

    func stop(_ why: String) {
        loop?.cancel()
        loop = nil
        hum?.stop()
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        status = why
    }

    /// Tells the brain which game this is (for this session, or the next).
    func tell(game: String) {
        let title = game.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let body = try? JSONSerialization.data(withJSONObject: ["title": title]),
              let r = Link.request("/v1/game", method: "POST", body: body, contentType: "application/json")
        else { return }
        session.dataTask(with: r).resume()
    }

    /// Lines can also come as notifications, when the app isn't running to say them.
    func askToNotify() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    private func listen() async {
        var after: UInt64?
        var failures = 0
        while !Task.isCancelled {
            var query: [String: String] = [:]
            if let after {
                query["after"] = String(after)
            }
            guard let r = Link.request("/v1/say", query, timeout: 40) else { return }
            do {
                let (data, response) = try await session.data(for: r)
                let code = (response as? HTTPURLResponse)?.statusCode ?? 0
                guard code == 200 else {
                    failures += 1
                    reachable = false
                    status = "Syrup's brain says: \(Link.explain(code, data))."
                    try? await Task.sleep(nanoseconds: 5_000_000_000)
                    continue
                }
                let answer = try JSONDecoder().decode(SayAnswer.self, from: data)
                failures = 0
                reachable = true
                watching = answer.watching
                if after != nil {
                    for line in answer.lines {
                        say(line.text)
                        lastLine = line.text
                    }
                }
                after = answer.last
                status = watching ? "Watching your game." : "Ready. Tap the round button, then Start Broadcast."
                // Nothing to watch for a while: stop, so the phone can rest.
                if watching || UIScreen.main.isCaptured {
                    quietSince = nil
                } else if let since = quietSince {
                    if Date().timeIntervalSince(since) > 300 {
                        stop("Stopped listening after five quiet minutes. Open Syrup to start again.")
                        return
                    }
                } else {
                    quietSince = Date()
                }
            } catch {
                if Task.isCancelled { return }
                failures += 1
                reachable = false
                status = "Can't reach Syrup's brain at \(Link.server?.host ?? "?"). Is your computer on?"
                let wait = UInt64(min(30, 2 * failures))
                try? await Task.sleep(nanoseconds: wait * 1_000_000_000)
            }
        }
    }

    private func say(_ text: String) {
        let s = AVAudioSession.sharedInstance()
        try? s.setCategory(.playback, mode: .spokenAudio, options: [.mixWithOthers, .duckOthers])
        try? s.setActive(true)
        let u = AVSpeechUtterance(string: text)
        u.voice = AVSpeechSynthesisVoice(language: "en-US")
        voice.speak(u)
    }

    /// Opens the audio session and starts the silent loop.
    private func stayAwake() {
        let s = AVAudioSession.sharedInstance()
        do {
            try s.setCategory(.playback, mode: .spokenAudio, options: [.mixWithOthers])
            try s.setActive(true)
        } catch {
            status = "Can't open the audio: \(error.localizedDescription)"
        }
        if hum == nil, let player = try? AVAudioPlayer(data: Mouth.silence()) {
            player.numberOfLoops = -1
            hum = player
        }
        hum?.play()
    }

    /// After a phone call or Siri: carry on.
    private func resume() {
        guard loop != nil else { return }
        stayAwake()
    }

    /// Syrup is done talking: the game's sound comes back up.
    fileprivate func finishedSpeaking() {
        guard !voice.isSpeaking, loop != nil else { return }
        let s = AVAudioSession.sharedInstance()
        hum?.pause()
        try? s.setActive(false, options: .notifyOthersOnDeactivation)
        try? s.setCategory(.playback, mode: .spokenAudio, options: [.mixWithOthers])
        try? s.setActive(true)
        hum?.play()
    }

    /// One second of silence, as a WAV file.
    static func silence() -> Data {
        let rate: UInt32 = 8000
        let samples = Data(count: Int(rate) * 2)
        var d = Data()
        func u32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        func u16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        d.append(contentsOf: Array("RIFF".utf8))
        u32(36 + UInt32(samples.count))
        d.append(contentsOf: Array("WAVE".utf8))
        d.append(contentsOf: Array("fmt ".utf8))
        u32(16)
        u16(1)
        u16(1)
        u32(rate)
        u32(rate * 2)
        u16(2)
        u16(16)
        d.append(contentsOf: Array("data".utf8))
        u32(UInt32(samples.count))
        d.append(samples)
        return d
    }
}

extension Mouth: AVSpeechSynthesizerDelegate {
    nonisolated func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 300_000_000)
            self?.finishedSpeaking()
        }
    }
}
