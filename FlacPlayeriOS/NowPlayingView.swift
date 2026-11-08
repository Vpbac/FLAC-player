import SwiftUI

public struct NowPlayingView: View {
    @ObservedObject var player = AudioPlayerManager.shared
    @ObservedObject var server = ServerManager.shared
    @Environment(\.presentationMode) var presentationMode
    @State private var isDraggingSlider = false
    @State private var dragSliderValue: Double = 0.0

    public var body: some View {
        ZStack {
            // Dark Gradient Background
            LinearGradient(
                gradient: Gradient(colors: [Color(red: 0.12, green: 0.14, blue: 0.18), Color(red: 0.06, green: 0.07, blue: 0.09)]),
                startPoint: .top,
                endPoint: .bottom
            )
            .ignoresSafeArea()

            VStack(spacing: 20) {
                // Top Dismiss Bar
                Capsule()
                    .fill(Color.gray.opacity(0.4))
                    .frame(width: 40, height: 5)
                    .padding(.top, 10)

                Spacer()

                // Album Artwork
                if let idx = player.currentIndex, let coverURL = server.coverURL(for: idx) {
                    AsyncImage(url: coverURL) { phase in
                        switch phase {
                        case .success(let image):
                            image
                                .resizable()
                                .aspectRatio(contentMode: .fill)
                        case .failure(_), .empty:
                            DefaultArtworkView()
                        @unknown default:
                            DefaultArtworkView()
                        }
                    }
                    .frame(width: 300, height: 300)
                    .clipShape(RoundedRectangle(cornerRadius: 18))
                    .shadow(color: Color.black.opacity(0.5), radius: 15, x: 0, y: 10)
                } else {
                    DefaultArtworkView()
                        .frame(width: 300, height: 300)
                        .clipShape(RoundedRectangle(cornerRadius: 18))
                        .shadow(color: Color.black.opacity(0.5), radius: 15, x: 0, y: 10)
                }

                Spacer()

                // Track Info & Badges
                VStack(spacing: 8) {
                    Text(player.currentTrack?.title ?? "Chưa phát bài nào")
                        .font(.system(size: 22, weight: .bold))
                        .foregroundColor(.white)
                        .lineLimit(1)

                    Text(player.currentTrack?.artist.isEmpty == false ? player.currentTrack!.artist : "Nghệ sĩ chưa rõ")
                        .font(.system(size: 16, weight: .medium))
                        .foregroundColor(.accentColor)
                        .lineLimit(1)

                    if let track = player.currentTrack {
                        HStack(spacing: 8) {
                            Text(track.hiResBadgeText)
                                .font(.system(size: 11, weight: .bold))
                                .foregroundColor(track.isHiRes ? Color.yellow : Color.gray)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 3)
                                .overlay(
                                    RoundedRectangle(cornerRadius: 4)
                                        .stroke(track.isHiRes ? Color.yellow : Color.gray, lineWidth: 1)
                                )

                            Text(track.formatLabel)
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundColor(.gray)
                        }
                        .padding(.top, 4)
                    }
                }
                .padding(.horizontal, 24)

                // Seek Progress Slider
                VStack(spacing: 6) {
                    Slider(
                        value: Binding(
                            get: { isDraggingSlider ? dragSliderValue : player.currentTime },
                            set: { val in
                                dragSliderValue = val
                                isDraggingSlider = true
                            }
                        ),
                        in: 0...max(player.duration, 0.001),
                        onEditingChanged: { editing in
                            if !editing {
                                player.seek(to: dragSliderValue)
                                isDraggingSlider = false
                            }
                        }
                    )
                    .accentColor(.white)

                    HStack {
                        Text(formatTime(isDraggingSlider ? dragSliderValue : player.currentTime))
                            .font(.system(size: 12, weight: .regular, design: .monospaced))
                            .foregroundColor(.gray)
                        Spacer()
                        Text(formatTime(player.duration))
                            .font(.system(size: 12, weight: .regular, design: .monospaced))
                            .foregroundColor(.gray)
                    }
                }
                .padding(.horizontal, 32)

                // Main Controls
                HStack(spacing: 40) {
                    Button(action: {
                        player.shuffle.toggle()
                    }) {
                        Image(systemName: "shuffle")
                            .font(.system(size: 20))
                            .foregroundColor(player.shuffle ? .accentColor : .gray)
                    }

                    Button(action: {
                        player.previous()
                    }) {
                        Image(systemName: "backward.fill")
                            .font(.system(size: 28))
                            .foregroundColor(.white)
                    }

                    Button(action: {
                        player.togglePlay()
                    }) {
                        Image(systemName: player.isPlaying ? "pause.circle.fill" : "play.circle.fill")
                            .font(.system(size: 64))
                            .foregroundColor(.white)
                    }

                    Button(action: {
                        player.next()
                    }) {
                        Image(systemName: "forward.fill")
                            .font(.system(size: 28))
                            .foregroundColor(.white)
                    }

                    Button(action: {
                        let nextMode = (player.repeatMode.rawValue + 1) % 3
                        player.repeatMode = RepeatMode(rawValue: nextMode) ?? .off
                    }) {
                        Image(systemName: player.repeatMode == .one ? "repeat.1" : "repeat")
                            .font(.system(size: 20))
                            .foregroundColor(player.repeatMode != .off ? .accentColor : .gray)
                    }
                }
                .padding(.vertical, 10)

                Spacer()
            }
        }
    }

    private func formatTime(_ seconds: Double) -> String {
        let sec = Int(seconds)
        let m = sec / 60
        let s = sec % 60
        return String(format: "%d:%02d", m, s)
    }
}

struct DefaultArtworkView: View {
    var body: some View {
        ZStack {
            Color(red: 0.16, green: 0.18, blue: 0.22)
            Image(systemName: "music.note")
                .font(.system(size: 80))
                .foregroundColor(Color.gray.opacity(0.4))
        }
    }
}
