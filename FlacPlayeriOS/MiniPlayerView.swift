import SwiftUI

public struct MiniPlayerView: View {
    @ObservedObject var player = AudioPlayerManager.shared
    @ObservedObject var server = ServerManager.shared
    var onTap: () -> Void

    public var body: some View {
        if let track = player.currentTrack {
            Button(action: onTap) {
                HStack(spacing: 12) {
                    if let idx = player.currentIndex, let coverURL = server.coverURL(for: idx) {
                        AsyncImage(url: coverURL) { phase in
                            if let img = phase.image {
                                img.resizable().aspectRatio(contentMode: .fill)
                            } else {
                                Image(systemName: "music.note")
                                    .foregroundColor(.gray)
                            }
                        }
                        .frame(width: 44, height: 44)
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                    } else {
                        RoundedRectangle(cornerRadius: 8)
                            .fill(Color.gray.opacity(0.3))
                            .frame(width: 44, height: 44)
                            .overlay(Image(systemName: "music.note").foregroundColor(.gray))
                    }

                    VStack(alignment: .leading, spacing: 2) {
                        Text(track.title)
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundColor(.white)
                            .lineLimit(1)

                        Text(track.artist.isEmpty ? track.formatLabel : "\(track.artist) • \(track.formatLabel)")
                            .font(.system(size: 12))
                            .foregroundColor(.gray)
                            .lineLimit(1)
                    }

                    Spacer()

                    Button(action: {
                        player.togglePlay()
                    }) {
                        Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                            .font(.system(size: 20))
                            .foregroundColor(.white)
                            .padding(.trailing, 8)
                    }

                    Button(action: {
                        player.next()
                    }) {
                        Image(systemName: "forward.fill")
                            .font(.system(size: 18))
                            .foregroundColor(.white)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .background(
                    RoundedRectangle(cornerRadius: 14)
                        .fill(Color(red: 0.16, green: 0.18, blue: 0.22))
                        .shadow(color: Color.black.opacity(0.4), radius: 8, x: 0, y: 4)
                )
                .padding(.horizontal, 12)
            }
            .buttonStyle(PlainButtonStyle())
        }
    }
}
