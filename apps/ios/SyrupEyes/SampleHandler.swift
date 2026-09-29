import CoreImage
import CoreMedia
import ImageIO
import ReplayKit
import UIKit
import UserNotifications

/// Syrup's eyes. While the player broadcasts their screen, a frame every half
/// second or so goes to Syrup's brain, scaled down and compressed as the
/// brain asks. What the brain says goes to the app, which says it; when the
/// app isn't listening, the line shows up as a notification instead.
///
/// Broadcast extensions get very little memory, so each frame is scaled and
/// compressed at once and only the latest one is kept.
final class SampleHandler: RPBroadcastSampleHandler {
    private let ci = CIContext(options: [.cacheIntermediates: false])
    private let sRGB = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
    private let queue = DispatchQueue(label: "syrup.eyes")
    private let session: URLSession = {
        let c = URLSessionConfiguration.default
        c.timeoutIntervalForRequest = 15
        return URLSession(configuration: c)
    }()

    // All below: only on `queue`.
    private var started = Date()
    private var busy = false
    private var lastSent = Date.distantPast
    private var lastFrame: Data?
    private var interval: TimeInterval = 0.5
    private var maxSide: CGFloat = 960
    private var quality: CGFloat = 0.6
    private var failures = 0
    private var reached = false
    private var finished = false
    private var timer: DispatchSourceTimer?

    override func broadcastStarted(withSetupInfo setupInfo: [String: NSObject]?) {
        guard Link.server != nil else {
            queue.async { self.finish("This build of Syrup has no server set.") }
            return
        }
        queue.async {
            self.started = Date()
            // A screen that stands still sends no frames: repeat the last one now
            // and then, so the brain knows the game is still on.
            let t = DispatchSource.makeTimerSource(queue: self.queue)
            t.schedule(deadline: .now() + 5, repeating: 5)
            t.setEventHandler { [weak self] in self?.heartbeat() }
            t.resume()
            self.timer = t
        }
    }

    override func broadcastFinished() {
        queue.sync {
            timer?.cancel()
            timer = nil
        }
        // Tell the brain now, so the summary is said at once.
        guard let r = Link.request("/v1/end", method: "POST", timeout: 4) else { return }
        let done = DispatchSemaphore(value: 0)
        session.dataTask(with: r) { _, _, _ in done.signal() }.resume()
        _ = done.wait(timeout: .now() + 4)
    }

    override func processSampleBuffer(_ sampleBuffer: CMSampleBuffer, with sampleBufferType: RPSampleBufferType) {
        guard sampleBufferType == .video else { return }
        let now = Date()
        let settings: (side: CGFloat, quality: CGFloat)? = queue.sync {
            guard !busy, !finished, now.timeIntervalSince(lastSent) >= interval else { return nil }
            busy = true
            lastSent = now
            return (side: maxSide, quality: quality)
        }
        guard let settings else { return }
        let jpeg: Data? = autoreleasepool {
            encode(sampleBuffer, side: settings.side, quality: settings.quality)
        }
        queue.async {
            guard let jpeg else {
                self.busy = false
                return
            }
            self.lastFrame = jpeg
            self.send(jpeg, at: now)
        }
    }

    private func encode(_ sampleBuffer: CMSampleBuffer, side: CGFloat, quality: CGFloat) -> Data? {
        guard let pixels = CMSampleBufferGetImageBuffer(sampleBuffer) else { return nil }
        var image = CIImage(cvPixelBuffer: pixels)
        if let n = CMGetAttachment(sampleBuffer, key: RPVideoSampleOrientationKey as CFString, attachmentModeOut: nil)
            as? NSNumber,
            let orientation = CGImagePropertyOrientation(rawValue: n.uint32Value)
        {
            image = image.oriented(orientation)
        }
        let longest = max(image.extent.width, image.extent.height)
        if longest > side {
            let s = side / longest
            image = image.transformed(by: CGAffineTransform(scaleX: s, y: s))
        }
        let q = CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String)
        return ci.jpegRepresentation(of: image, colorSpace: sRGB, options: [q: quality])
    }

    /// On `queue`.
    private func send(_ jpeg: Data, at when: Date) {
        let t = max(0, Int(when.timeIntervalSince(started) * 1000))
        guard let r = Link.request("/v1/frame", ["t": String(t)], method: "POST", body: jpeg, contentType: "image/jpeg")
        else {
            busy = false
            return
        }
        session.dataTask(with: r) { [weak self] data, response, error in
            guard let self else { return }
            self.queue.async { self.answered(data, response as? HTTPURLResponse, error) }
        }.resume()
    }

    /// On `queue`.
    private func answered(_ data: Data?, _ response: HTTPURLResponse?, _ error: Error?) {
        busy = false
        guard let response, error == nil else {
            failures += 1
            if !reached && failures >= 10 {
                let host = Link.server?.host ?? "?"
                finish("Can't reach Syrup's brain at \(host). Is your computer on, and Syrup running?")
            }
            return
        }
        guard response.statusCode == 200, let data,
              let answer = try? JSONDecoder().decode(FrameAnswer.self, from: data)
        else {
            if response.statusCode == 401 || response.statusCode == 403 {
                finish("Syrup's brain says: \(Link.explain(response.statusCode, data)).")
            }
            failures += 1
            return
        }
        failures = 0
        reached = true
        interval = max(0.2, Double(answer.next.interval_ms) / 1000)
        maxSide = CGFloat(max(320, min(2048, answer.next.max_side)))
        quality = CGFloat(max(0.2, min(0.95, answer.next.quality)))
        if !answer.mouth {
            for line in answer.say {
                notify(line)
            }
        }
    }

    /// On `queue`.
    private func heartbeat() {
        guard !busy, !finished, let frame = lastFrame, Date().timeIntervalSince(lastSent) >= 5 else { return }
        busy = true
        lastSent = Date()
        send(frame, at: lastSent)
    }

    private func notify(_ text: String) {
        let content = UNMutableNotificationContent()
        content.title = "Syrup"
        content.body = text
        content.sound = .default
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    /// On `queue`.
    private func finish(_ why: String) {
        guard !finished else { return }
        finished = true
        timer?.cancel()
        timer = nil
        finishBroadcastWithError(NSError(domain: "Syrup", code: 1, userInfo: [NSLocalizedDescriptionKey: why]))
    }
}
