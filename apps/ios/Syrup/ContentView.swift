import ReplayKit
import SwiftUI

private let brown = Color(red: 0x57 / 255, green: 0x35 / 255, blue: 0x1F / 255)
private let cream = Color(red: 1.0, green: 0.97, blue: 0.9)
private let honey = Color(red: 0.96, green: 0.68, blue: 0.24)

struct ContentView: View {
    @EnvironmentObject private var mouth: Mouth
    @Environment(\.scenePhase) private var phase
    @AppStorage("game") private var game = ""

    /// Green when watching, honey when ready, red when the brain can't be reached.
    private var light: Color {
        switch mouth.reachable {
        case .some(false): return Color(red: 0.98, green: 0.8, blue: 0.78)
        case .some(true): return mouth.watching ? Color(red: 0.8, green: 0.93, blue: 0.78) : honey.opacity(0.35)
        case .none: return Color.white
        }
    }

    var body: some View {
        ZStack {
            cream.ignoresSafeArea()
            ScrollView {
                VStack(spacing: 16) {
                    Image("SyrupFace")
                        .resizable()
                        .scaledToFit()
                        .frame(width: 160)
                        .accessibilityHidden(true)
                    Text("Syrup")
                        .font(.system(size: 40, weight: .heavy, design: .rounded))
                        .foregroundColor(brown)
                    Text("Learns the game with you.")
                        .font(.headline)
                        .foregroundColor(brown.opacity(0.75))
                    TextField("What are you playing? (optional)", text: $game)
                        .textInputAutocapitalization(.words)
                        .disableAutocorrection(true)
                        .submitLabel(.done)
                        .padding(12)
                        .background(RoundedRectangle(cornerRadius: 12).fill(Color.white))
                        .overlay(RoundedRectangle(cornerRadius: 12).stroke(brown.opacity(0.25)))
                        .onSubmit { mouth.tell(game: game) }
                    Text(mouth.status)
                        .font(.subheadline.weight(.semibold))
                        .multilineTextAlignment(.center)
                        .foregroundColor(brown)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 8)
                        .background(Capsule().fill(light))
                    BroadcastButton()
                        .frame(width: 96, height: 96)
                        .background(Circle().fill(honey))
                        .overlay(Circle().stroke(brown, lineWidth: 3))
                        .accessibilityLabel("Start watching")
                    Text("Tap it, choose Start Broadcast, and go play. Syrup talks to you while you play. To stop, tap the red mark at the top of the screen.")
                        .font(.footnote)
                        .multilineTextAlignment(.center)
                        .foregroundColor(brown.opacity(0.8))
                    if let line = mouth.lastLine {
                        Text("“\(line)”")
                            .italic()
                            .multilineTextAlignment(.center)
                            .foregroundColor(brown)
                    }
                    Text(
                        "While you broadcast, Syrup sees your whole screen, notifications included. Each picture goes to your Syrup server, is looked at, and isn't kept."
                    )
                    .font(.caption2)
                    .multilineTextAlignment(.center)
                    .foregroundColor(brown.opacity(0.6))
                    .padding(.top, 8)
                }
                .padding(24)
            }
        }
        .onAppear {
            mouth.askToNotify()
            mouth.start()
            if !game.isEmpty {
                mouth.tell(game: game)
            }
        }
        .onChange(of: phase) { now in
            if now == .active {
                mouth.start()
            }
        }
    }
}

/// Apple's own "start broadcasting" button, pointed at Syrup's eyes.
struct BroadcastButton: UIViewRepresentable {
    func makeUIView(context: Context) -> RPSystemBroadcastPickerView {
        let picker = RPSystemBroadcastPickerView(frame: CGRect(x: 0, y: 0, width: 84, height: 84))
        picker.preferredExtension = (Bundle.main.bundleIdentifier ?? "") + ".eyes"
        picker.showsMicrophoneButton = false
        let ink = UIColor(red: 0x57 / 255, green: 0x35 / 255, blue: 0x1F / 255, alpha: 1)
        picker.tintColor = ink
        for case let button as UIButton in picker.subviews {
            button.tintColor = ink
            button.imageView?.tintColor = ink
        }
        return picker
    }

    func updateUIView(_ uiView: RPSystemBroadcastPickerView, context: Context) {}
}
