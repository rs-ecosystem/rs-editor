import Foundation

/// The title of a selection menu item (`cut`, `copy`, `paste`,
/// `selectAll`) in the user's language.
func menuLabel(_ item: String) -> String {
    let indonesian = Locale.preferredLanguages.first?.hasPrefix("id") == true
    switch item {
    case "cut": return indonesian ? "Potong" : "Cut"
    case "copy": return indonesian ? "Salin" : "Copy"
    case "paste": return indonesian ? "Tempel" : "Paste"
    default: return indonesian ? "Pilih Semua" : "Select All"
    }
}
