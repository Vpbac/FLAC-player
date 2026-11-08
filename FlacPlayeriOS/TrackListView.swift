import SwiftUI

struct ItemTrack: Identifiable {
    var id: String { track.id }
    let index: Int
    let track: Track
}

public struct TrackListView: View {
    @ObservedObject var server = ServerManager.shared
    @ObservedObject var player = AudioPlayerManager.shared
    @State private var searchText: String = ""
    @State private var showSettings: Bool = false
    @State private var showNowPlaying: Bool = false

    var filteredTracks: [ItemTrack] {
        let all = server.tracks.enumerated().map { ItemTrack(index: $0.offset, track: $0.element) }
        let q = searchText.trimmingCharacters(in: .whitespaces).lowercased()
        if q.isEmpty {
            return all
        }
        return all.filter { item in
            item.track.title.lowercased().contains(q) ||
            item.track.artist.lowercased().contains(q) ||
            item.track.album.lowercased().contains(q)
        }
    }

    public var body: some View {
        NavigationView {
            ZStack(alignment: .bottom) {
                Color(red: 0.08, green: 0.09, blue: 0.11)
                    .ignoresSafeArea()

                VStack(spacing: 0) {
                    if !server.isConnected {
                        ConnectionBannerView(showSettings: $showSettings)
                    }

                    if server.tracks.isEmpty {
                        VStack(spacing: 16) {
                            Spacer()
                            Image(systemName: "music.note.list")
                                .font(.system(size: 54))
                                .foregroundColor(.gray.opacity(0.5))

                            Text("Chưa có bài hát nào")
                                .font(.system(size: 18, weight: .semibold))
                                .foregroundColor(.white)

                            Text("Hãy kéo thả nhạc vào app trên PC hoặc kiểm tra kết nối.")
                                .font(.system(size: 14))
                                .foregroundColor(.gray)
                                .multilineTextAlignment(.center)
                                .padding(.horizontal, 32)

                            Button(action: {
                                server.fetchTracks()
                            }) {
                                HStack {
                                    Image(systemName: "arrow.clockwise")
                                    Text("Làm mới")
                                }
                                .padding(.horizontal, 16)
                                .padding(.vertical, 8)
                                .background(Color.blue)
                                .foregroundColor(.white)
                                .clipShape(Capsule())
                            }
                            Spacer()
                        }
                    } else {
                        List {
                            ForEach(filteredTracks) { item in
                                TrackRowView(
                                    track: item.track,
                                    index: item.index,
                                    isPlayingThis: player.currentTrack?.id == item.track.id && player.isPlaying
                                )
                                .contentShape(Rectangle())
                                .onTapGesture {
                                    player.play(track: item.track, index: item.index)
                                }
                            }
                            .listRowBackground(Color(red: 0.11, green: 0.12, blue: 0.15))
                        }
                        .listStyle(PlainListStyle())
                    }
                }

                // Mini Player Bar
                MiniPlayerView {
                    showNowPlaying = true
                }
                .padding(.bottom, 8)
            }
            .navigationTitle("Hi-Res Library")
            .navigationBarItems(
                leading: Button(action: {
                    server.fetchTracks()
                }) {
                    Image(systemName: "arrow.clockwise")
                },
                trailing: Button(action: {
                    showSettings = true
                }) {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(server.isConnected ? Color.green : Color.red)
                            .frame(width: 8, height: 8)
                        Image(systemName: "gearshape")
                    }
                }
            )
            .sheet(isPresented: $showSettings) {
                SettingsView()
            }
            .sheet(isPresented: $showNowPlaying) {
                NowPlayingView()
            }
        }
    }
}

struct ConnectionBannerView: View {
    @Binding var showSettings: Bool

    var body: some View {
        HStack {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundColor(.yellow)
            Text("Chưa kết nối PC. Chạm để cài đặt IP.")
                .font(.system(size: 13, weight: .medium))
                .foregroundColor(.white)
            Spacer()
            Image(systemName: "chevron.right")
                .foregroundColor(.gray)
                .font(.system(size: 12))
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(Color.red.opacity(0.85))
        .onTapGesture {
            showSettings = true
        }
    }
}

struct TrackRowView: View {
    let track: Track
    let index: Int
    let isPlayingThis: Bool

    var body: some View {
        HStack(spacing: 12) {
            Text("\(index + 1)")
                .font(.system(size: 13, weight: .semibold, design: .monospaced))
                .foregroundColor(isPlayingThis ? .accentColor : .gray)
                .frame(width: 24)

            VStack(alignment: .leading, spacing: 3) {
                Text(track.title)
                    .font(.system(size: 15, weight: isPlayingThis ? .bold : .medium))
                    .foregroundColor(isPlayingThis ? .accentColor : .white)
                    .lineLimit(1)

                HStack(spacing: 6) {
                    Text(track.artist.isEmpty ? "—" : track.artist)
                        .font(.system(size: 13))
                        .foregroundColor(.gray)
                        .lineLimit(1)

                    Text("•")
                        .foregroundColor(.gray)
                        .font(.system(size: 10))

                    Text(track.formatLabel)
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundColor(track.isHiRes ? .yellow : .gray)
                }
            }

            Spacer()

            if isPlayingThis {
                Image(systemName: "waveform")
                    .foregroundColor(.accentColor)
                    .font(.system(size: 14))
            }

            Text(formatDuration(track.duration))
                .font(.system(size: 12, design: .monospaced))
                .foregroundColor(.gray)
        }
        .padding(.vertical, 4)
    }

    private func formatDuration(_ s: Double) -> String {
        let sec = Int(s)
        return String(format: "%d:%02d", sec / 60, sec % 60)
    }
}
