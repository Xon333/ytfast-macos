import AppKit

struct Song: Codable, Equatable { var id: String; var title: String; var artist: String }
struct Profile: Codable, Equatable { var id: String; var label: String }
struct Row: Codable, Equatable {
    var title: String; var subtitle: String
    var play: String?; var browse: String?; var video: String?; var editable: String?
    var identity: String { browse ?? play ?? editable ?? title }
}
struct Page: Codable, Equatable {
    var key: String; var target: String; var title: String; var rows: [Row]
    var play: String?; var loading: Bool; var more: Bool; var message: String?
}
struct State: Codable {
    var track: Song?; var playing = false; var loading = false
    var position = 0.0; var duration = 0.0; var volume = 70.0; var shuffle = false
    var format: String?; var signed_in = false; var account_checking = true
    var account = "Connecting…"; var profiles: [Profile] = []; var profile: String?
    var pages: [Page]?; var notice: String?; var error: String?
    var adding = false; var show = false; var quit = false
}

protocol PlayerAPI { func send(_ action: [String: Any]) -> State? }

func consume(_ pointer: UnsafeMutablePointer<CChar>?) -> Data {
    guard let pointer else { return Data() }
    defer { ytfast_free(pointer) }
    return Data(String(cString: pointer).utf8)
}

final class CoreAPI: PlayerAPI {
    func send(_ action: [String: Any]) -> State? {
        guard let bytes = try? JSONSerialization.data(withJSONObject: action),
              let text = String(data: bytes, encoding: .utf8) else { return nil }
        let response = text.withCString { consume(ytfast_call($0)) }
        if let state = try? JSONDecoder().decode(State.self, from: response) { return state }
        if let error = (try? JSONSerialization.jsonObject(with: response)) as? [String: Any] {
            fputs("YTfast: \(error["fatal"] as? String ?? "Invalid bridge response")\n", stderr)
        }
        return nil
    }
}

func encodeTarget(_ kind: String, _ values: [String: Any]) -> String {
    let data = try! JSONSerialization.data(withJSONObject: [kind: values], options: [.sortedKeys])
    return String(decoding: data, as: UTF8.self)
}
func browseTarget(_ id: String) -> String { encodeTarget("Browse", ["id": id, "params": NSNull()]) }
let playlistID = "FEmusic_liked_playlists"
let playlistKey = "browse:\(playlistID):"

struct Location {
    var target: String; var key: String; var title: String
    var song: Song?; var scroll = NSPoint.zero
    var identity: String { key + (song.map { ":add:\($0.id)" } ?? "") }
    static func library(_ index: Int) -> Location {
        let choices = [(playlistID, "Playlists"), ("FEmusic_liked_videos", "Liked Music"), ("FEmusic_liked_albums", "Albums")]
        let choice = choices[max(0, min(index, choices.count - 1))]
        return Location(target: browseTarget(choice.0), key: "browse:\(choice.0):", title: choice.1)
    }
    static func from(_ target: String, title: String) -> Location? {
        guard let object = try? JSONSerialization.jsonObject(with: Data(target.utf8)) as? [String: Any] else { return nil }
        if let body = object["Browse"] as? [String: Any], let id = body["id"] as? String {
            return Location(target: target, key: "browse:\(id):\(body["params"] as? String ?? "")", title: title)
        }
        if let body = object["Search"] as? [String: Any], let query = body["query"] as? String {
            return Location(target: target, key: "search:\(query):\(body["params"] as? String ?? "")", title: title)
        }
        return nil
    }
}

final class ActionBox: NSObject {
    let value: [String: Any]
    init(_ value: [String: Any]) { self.value = value }
}

func setText(_ label: NSTextField, _ value: String) {
    if label.stringValue != value { label.stringValue = value }
}
func timeLabel(_ seconds: Double) -> String {
    guard seconds.isFinite else { return "0:00" }
    let value = max(0, Int(seconds))
    return value >= 3600 ? String(format: "%d:%02d:%02d", value / 3600, value / 60 % 60, value % 60)
        : String(format: "%d:%02d", value / 60, value % 60)
}
