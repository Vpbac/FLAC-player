import Foundation
import AVFoundation
import MediaPlayer
import Combine

public enum RepeatMode: Int {
    case off = 0
    case all = 1
    case one = 2

    public var title: String {
        switch self {
        case .off: return "Tắt"
        case .all: return "Tất cả"
        case .one: return "Một bài"
        }
    }
}

public class AudioPlayerManager: NSObject, ObservableObject {
    public static let shared = AudioPlayerManager()

    private var player: AVPlayer?
    private var timeObserverToken: Any?

    @Published public var currentTrack: Track?
    @Published public var currentIndex: Int?
    @Published public var isPlaying: Bool = false
    @Published public var currentTime: Double = 0.0
    @Published public var duration: Double = 0.0
    @Published public var isBuffering: Bool = false
    @Published public var shuffle: Bool = false
    @Published public var repeatMode: RepeatMode = .all

    private var shuffleIndices: [Int] = []

    override init() {
        super.init()
        setupAudioSession()
        setupRemoteCommands()
    }

    private func setupAudioSession() {
        do {
            try AVAudioSession.sharedInstance().setCategory(.playback, mode: .default, options: [])
            try AVAudioSession.sharedInstance().setActive(true)
        } catch {
            print("Failed to set audio session category: \(error)")
        }
    }

    private func setupRemoteCommands() {
        let commandCenter = MPRemoteCommandCenter.shared()

        commandCenter.playCommand.addTarget { [weak self] _ in
            self?.resume()
            return .success
        }
        commandCenter.pauseCommand.addTarget { [weak self] _ in
            self?.pause()
            return .success
        }
        commandCenter.togglePlayPauseCommand.addTarget { [weak self] _ in
            self?.togglePlay()
            return .success
        }
        commandCenter.nextTrackCommand.addTarget { [weak self] _ in
            self?.next()
            return .success
        }
        commandCenter.previousTrackCommand.addTarget { [weak self] _ in
            self?.previous()
            return .success
        }
        commandCenter.changePlaybackPositionCommand.addTarget { [weak self] event in
            if let posEvent = event as? MPChangePlaybackPositionCommandEvent {
                self?.seek(to: posEvent.positionTime)
                return .success
            }
            return .commandFailed
        }
    }

    public func play(track: Track, index: Int) {
        guard let streamURL = ServerManager.shared.streamURL(for: index) else { return }

        self.currentTrack = track
        self.currentIndex = index
        self.duration = track.duration
        self.currentTime = 0

        // Reset previous player
        if let token = timeObserverToken {
            player?.removeTimeObserver(token)
            timeObserverToken = nil
        }
        NotificationCenter.default.removeObserver(self, name: .AVPlayerItemDidPlayToEndTime, object: nil)

        let playerItem = AVPlayerItem(url: streamURL)
        if player == nil {
            player = AVPlayer(playerItem: playerItem)
        } else {
            player?.replaceCurrentItem(with: playerItem)
        }

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(itemDidFinishPlaying),
            name: .AVPlayerItemDidPlayToEndTime,
            object: playerItem
        )

        // Periodic time observer (every 0.25s)
        let interval = CMTime(seconds: 0.25, preferredTimescale: 600)
        timeObserverToken = player?.addPeriodicTimeObserver(forInterval: interval, queue: .main) { [weak self] time in
            guard let self = self else { return }
            self.currentTime = time.seconds
            self.updateNowPlayingInfo()
        }

        player?.play()
        self.isPlaying = true
        updateNowPlayingInfo()
    }

    public func togglePlay() {
        if isPlaying {
            pause()
        } else {
            resume()
        }
    }

    public func pause() {
        player?.pause()
        self.isPlaying = false
        updateNowPlayingInfo()
    }

    public func resume() {
        if let player = player {
            player.play()
            self.isPlaying = true
            updateNowPlayingInfo()
        } else if let track = currentTrack, let idx = currentIndex {
            play(track: track, index: idx)
        }
    }

    public func seek(to seconds: Double) {
        let time = CMTime(seconds: seconds, preferredTimescale: 600)
        player?.seek(to: time, toleranceBefore: .zero, toleranceAfter: .zero)
        self.currentTime = seconds
        updateNowPlayingInfo()
    }

    public func next() {
        let tracks = ServerManager.shared.tracks
        guard !tracks.isEmpty else { return }
        guard let current = currentIndex else {
            if let first = tracks.first { play(track: first, index: 0) }
            return
        }

        if repeatMode == .one {
            seek(to: 0)
            resume()
            return
        }

        let nextIdx: Int
        if shuffle {
            nextIdx = Int.random(in: 0..<tracks.count)
        } else {
            nextIdx = (current + 1) % tracks.count
        }

        play(track: tracks[nextIdx], index: nextIdx)
    }

    public func previous() {
        let tracks = ServerManager.shared.tracks
        guard !tracks.isEmpty else { return }

        if currentTime > 3.0 {
            seek(to: 0)
            return
        }

        guard let current = currentIndex else { return }
        let prevIdx = (current + tracks.count - 1) % tracks.count
        play(track: tracks[prevIdx], index: prevIdx)
    }

    @objc private func itemDidFinishPlaying() {
        next()
    }

    private func updateNowPlayingInfo() {
        guard let track = currentTrack else {
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            return
        }

        var nowPlayingInfo = [String: Any]()
        nowPlayingInfo[MPMediaItemPropertyTitle] = track.title
        nowPlayingInfo[MPMediaItemPropertyArtist] = track.artist.isEmpty ? "Hi-Res Audio" : track.artist
        nowPlayingInfo[MPMediaItemPropertyAlbumTitle] = track.album
        nowPlayingInfo[MPMediaItemPropertyPlaybackDuration] = track.duration > 0 ? track.duration : self.duration
        nowPlayingInfo[MPNowPlayingInfoPropertyElapsedPlaybackTime] = self.currentTime
        nowPlayingInfo[MPNowPlayingInfoPropertyPlaybackRate] = isPlaying ? 1.0 : 0.0

        // Fetch artwork asynchronously if available
        if let idx = currentIndex, let coverURL = ServerManager.shared.coverURL(for: idx) {
            URLSession.shared.dataTask(with: coverURL) { data, _, _ in
                if let data = data, let image = UIImage(data: data) {
                    let artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in image }
                    DispatchQueue.main.async {
                        var info = MPNowPlayingInfoCenter.default().nowPlayingInfo ?? [:]
                        info[MPMediaItemPropertyArtwork] = artwork
                        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
                    }
                }
            }.resume()
        }

        MPNowPlayingInfoCenter.default().nowPlayingInfo = nowPlayingInfo
    }
}
