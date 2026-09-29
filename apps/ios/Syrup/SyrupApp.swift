import SwiftUI

@main
struct SyrupApp: App {
    @StateObject private var mouth = Mouth()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(mouth)
        }
    }
}
