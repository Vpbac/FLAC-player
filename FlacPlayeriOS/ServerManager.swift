import Foundation
import Combine

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
    @Published public var errorMessage: String? = nil

    private var cancellables = Set<AnyCancellable>()

    private init() {
        let saved = UserDefaults.standard.string(forKey: "saved_server_url") ?? "http://192.168.1.10:8080"
        self.serverURLString = saved
        testConnection()
    }

    public var cleanBaseURL: String {
        var str = serverURLString.trimmingCharacters(in: .whitespacesAndNewlines)
        if !str.lowercased().hasPrefix("http://") && !str.lowercased().hasPrefix("https://") {
            str = "http://" + str
        }
        return str.hasSuffix("/") ? String(str.dropLast()) : str
    }

    public func streamURL(for index: Int) -> URL? {
        return URL(string: "\(cleanBaseURL)/api/stream/\(index)")
    }

    public func coverURL(for index: Int) -> URL? {
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
        request.timeoutInterval = 3.5

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
                    self?.errorMessage = "Không kết nối được tới PC. Hãy kiểm tra Wi-Fi và IP."
                }
            }
            .store(in: &cancellables)
    }

    public func fetchTracks() {
        guard let url = URL(string: "\(cleanBaseURL)/api/tracks") else { return }

        var request = URLRequest(url: url)
        request.timeoutInterval = 8.0

        URLSession.shared.dataTaskPublisher(for: request)
            .map { $0.data }
            .decode(type: [Track].self, decoder: JSONDecoder())
            .receive(on: DispatchQueue.main)
            .sink(receiveCompletion: { [weak self] completion in
                if case .failure(let err) = completion {
                    self?.errorMessage = "Lỗi nạp danh sách: \(err.localizedDescription)"
                }
            }, receiveValue: { [weak self] fetched in
                self?.tracks = fetched
                self?.errorMessage = nil
            })
            .store(in: &cancellables)
    }
}
