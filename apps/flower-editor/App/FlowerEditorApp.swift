import SwiftUI

@main
struct FlowerEditorApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                #if os(macOS)
                .frame(minWidth: 420, idealWidth: 560, minHeight: 320, idealHeight: 640)
                #endif
        }
    }
}
