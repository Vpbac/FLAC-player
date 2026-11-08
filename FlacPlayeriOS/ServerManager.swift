import Foundation
import Combine
import CryptoKit

public class ServerManager: ObservableObject {
    public static let shared = ServerManager()

    @Published public var serverURLString: String {
        didSet {
            UserDefaults.standard.set(serverURLString, forKey: "saved_server_url")
            testConnection()
        }
    }

    @Published public var isConnected: Bool = false
    @Published public var isConnecting: Bool = false
    @Published public var tracks: [Track] = []
    @Published public var cachedPaths: Set<String> = []
    @Published public var isSyncingOffline: Bool = false
    @Published public var errorMessage: String? = nil

    private var cancellables = Set<AnyCancellable>()
    private let fileManager = FileManager.default

    private var offlineAudioDir: URL {
        let docs = fileManager.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let dir = docs.appendingPathComponent("OfflineAudio", isDirectory: true)
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    private var offlineCoverDir: URL {
        let docs = fileManager.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let dir = docs.appendingPathComponent("OfflineCovers", isDirectory: true)
        try? fileManager.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    private var cachedTracksFileURL: URL {
        let docs = fileManager.urls(for: .documentDirectory, in: .userDomainMask)[0]
        return docs.appendingPathComponent("cached_tracks.json")
    }

    private init() {
        let saved = UserDefaults.standard.string(forKey: "saved_server_url") ?? "http://192.168.1.10:8080"
        self.serverURLString = saved
        loadCachedMetadata()
        refreshCachedPaths()
        testConnection()

        Timer.publish(every: 3.0, on: .main, in: .common)
            .autoconnect()
            .sink { [weak self] _ in
                guard let self = self else { return }
                self.fetchTracksQuietly()
            }
            .store(in: &cancellables)
    }

    private func hashKey(for path: String) -> String {
        let digest = SHA256.hash(data: Data(path.utf8))
        return digest.map { String(format: "%02x", $0) }.joined()
    }

    public func localAudioURL(for track: Track) -> URL {
        let ext = (track.path as NSString).pathExtension.isEmpty ? "flac" : (track.path as NSString).pathExtension
        return offlineAudioDir.appendingPathComponent("\(hashKey(for: track.path)).\(ext)")
    }

    public func localCoverURL(for track: Track) -> URL {
        return offlineCoverDir.appendingPathComponent("\(hashKey(for: track.path)).jpg")
    }

    public func isTrackCached(_ track: Track) -> Bool {
        return cachedPaths.contains(track.path)
    }

    private func loadCachedMetadata() {
        if let data = try? Data(contentsOf: cachedTracksFileURL),
           let decoded = try? JSONDecoder().decode([Track].self, from: data) {
            self.tracks = decoded
        }
    }

    private func saveCachedMetadata(_ list: [Track]) {
        if let data = try? JSONEncoder().encode(list) {
            try? data.write(to: cachedTracksFileURL, options: .atomic)
        }
    }

    public func refreshCachedPaths() {
        var found = Set<String>()
        for t in tracks {
            let url = localAudioURL(for: t)
            if fileManager.fileExists(atPath: url.path) {
                found.insert(t.path)
            }
        }
        DispatchQueue.main.async {
            self.cachedPaths = found
        }
    }

    public var cleanBaseURL: String {
        var str = serverURLString.trimmingCharacters(in: .whitespacesAndNewlines)
        if !str.lowercased().hasPrefix("http://") && !str.lowercased().hasPrefix("https://") {
            str = "http://" + str
        }
        return str.hasSuffix("/") ? String(str.dropLast()) : str
    }

    public func streamURL(for index: Int) -> URL? {
        if index >= 0 && index < tracks.count {
            let t = tracks[index]
            let local = localAudioURL(for: t)
            if fileManager.fileExists(atPath: local.path) {
                return local
            }
        }
        return URL(string: "\(cleanBaseURL)/api/stream/\(index)")
    }

    public func coverURL(for index: Int) -> URL? {
        if index >= 0 && index < tracks.count {
            let t = tracks[index]
            let local = localCoverURL(for: t)
            if fileManager.fileExists(atPath: local.path) {
                return local
            }
        }
        return URL(string: "\(cleanBaseURL)/api/cover/\(index)")
    }

    public func testConnection() {
        guard let url = URL(string: "\(cleanBaseURL)/api/status") else {
            self.isConnected = false
            self.errorMessage = "Địa chỉ IP không hợp lệ"
            return
        }

        self.isConnecting = true
        self.errorMessage = nil

        var request = URLRequest(url: url)
        request.timeoutInterval = 3.0

        URLSession.shared.dataTaskPublisher(for: request)
            .map { _ in true }
            .replaceError(with: false)
            .receive(on: DispatchQueue.main)
            .sink { [weak self] ok in
                self?.isConnecting = false
                self?.isConnected = ok
                if ok {
                    self?.fetchTracks()
                } else {
                    self?.errorMessage = "PC đang tắt — Đang phát nhạc từ bộ nhớ Offline của iPhone."
                }
            }
            .store(in: &cancellables)
    }

    public func fetchTracks() {
        guard let url = URL(string: "\(cleanBaseURL)/api/tracks?t=\(Int(Date().timeIntervalSince1970))") else { return }

        var request = URLRequest(url: url)
        request.timeoutInterval = 5.0

        URLSession.shared.dataTaskPublisher(for: request)
            .map { $0.data }
            .decode(type: [Track].self, decoder: JSONDecoder())
            .receive(on: DispatchQueue.main)
            .sink(receiveCompletion: { [weak self] completion in
                if case .failure(_) = completion {
                    self?.isConnected = false
                }
            }, receiveValue: { [weak self] fetched in
                guard let self = self else { return }
                self.isConnected = true
                self.tracks = fetched
                self.errorMessage = nil
                self.saveCachedMetadata(fetched)
                self.refreshCachedPaths()
                self.syncOfflineTracksIfNeeded()
            })
            .store(in: &cancellables)
    }

    public func fetchTracksQuietly() {
        guard let url = URL(string: "\(cleanBaseURL)/api/tracks?t=\(Int(Date().timeIntervalSince1970))") else { return }

        var request = URLRequest(url: url)
        request.timeoutInterval = 2.5

        URLSession.shared.dataTaskPublisher(for: request)
            .map { $0.data }
            .decode(type: [Track].self, decoder: JSONDecoder())
            .receive(on: DispatchQueue.main)
            .sink(receiveCompletion: { [weak self] completion in
                if case .failure(_) = completion {
                    self?.isConnected = false
                }
            }, receiveValue: { [weak self] fetched in
                guard let self = self else { return }
                let wasOffline = !self.isConnected
                self.isConnected = true
                if self.tracks != fetched || wasOffline {
                    self.tracks = fetched
                    self.saveCachedMetadata(fetched)
                    self.refreshCachedPaths()
                }
                self.syncOfflineTracksIfNeeded()
            })
            .store(in: &cancellables)
    }

    public func syncOfflineTracksIfNeeded() {
        guard isConnected && !isSyncingOffline && !tracks.isEmpty else { return }
        let snapshot = tracks
        let baseURL = cleanBaseURL

        isSyncingOffline = true
        DispatchQueue.global(qos: .utility).async { [weak self] in
            guard let self = self else { return }
            for (index, track) in snapshot.enumerated() {
                let targetAudio = self.localAudioURL(for: track)
                if !self.fileManager.fileExists(atPath: targetAudio.path) {
                    if let remoteAudioURL = URL(string: "\(baseURL)/api/stream/\(index)"),
                       let data = try? Data(contentsOf: remoteAudioURL),
                       !data.isEmpty {
                        try? data.write(to: targetAudio, options: .atomic)
                        DispatchQueue.main.async {
                            self.cachedPaths.insert(track.path)
                        }
                    }
                }
                let targetCover = self.localCoverURL(for: track)
                if !self.fileManager.fileExists(atPath: targetCover.path) {
                    if let remoteCoverURL = URL(string: "\(baseURL)/api/cover/\(index)"),
                       let cData = try? Data(contentsOf: remoteCoverURL),
                       !cData.isEmpty {
                        try? cData.write(to: targetCover, options: .atomic)
                    }
                }
            }
            DispatchQueue.main.async {
                self.isSyncingOffline = false
                self.refreshCachedPaths()
            }
        }
    }
}
