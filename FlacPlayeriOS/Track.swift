import Foundation

public struct Track: Identifiable, Codable, Equatable {
    public var id: String { path }
    public let path: String
    public let title: String
    public let artist: String
    public let album: String
    public let trackNo: Int
    public let duration: Double
    public let sampleRate: Int
    public let bits: Int
    public let channels: Int
    public let bitrate: Int

    enum CodingKeys: String, CodingKey {
        case path
        case title
        case artist
        case album
        case trackNo = "track_no"
        case duration
        case sampleRate = "sample_rate"
        case bits
        case channels
        case bitrate
    }

    public var isHiRes: Bool {
        return bits >= 24 || sampleRate > 48000
    }

    public var formatLabel: String {
        let ext = (path as NSString).pathExtension.uppercased()
        let rateStr: String
        if sampleRate % 1000 == 0 {
            rateStr = "\(sampleRate / 1000)"
        } else {
            rateStr = String(format: "%.1f", Double(sampleRate) / 1000.0)
        }

        if bits > 0 {
            return "\(ext) \(bits)/\(rateStr)kHz"
        } else if bitrate > 0 {
            return "\(ext) \(rateStr)kHz \(bitrate)kbps"
        } else {
            return ext
        }
    }

    public var hiResBadgeText: String {
        if bits >= 24 && sampleRate >= 96000 {
            return "HI-RES AUDIO"
        } else if bits >= 24 || sampleRate > 48000 {
            return "LOSSLESS 24-BIT"
        } else {
            return "LOSSLESS"
        }
    }
}
