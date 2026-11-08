import Foundation
import AVFoundation
import Combine

public enum EqFilterType: String, CaseIterable, Codable {
    case peaking = "PK"
    case lowShelf = "LSC"
    case highShelf = "HSC"
    case lowPass = "LPQ"
    case highPass = "HPQ"
    case notch = "NO"
    case bandPass = "BP"

    public var label: String {
        switch self {
        case .peaking: return "PK (Peaking)"
        case .lowShelf: return "LSC (Low Shelf)"
        case .highShelf: return "HSC (High Shelf)"
        case .lowPass: return "LP (Low Pass)"
        case .highPass: return "HP (High Pass)"
        case .notch: return "NO (Notch)"
        case .bandPass: return "BP (Band Pass)"
        }
    }

    public static func fromToken(_ token: String) -> EqFilterType? {
        switch token.uppercased() {
        case "PK", "PEA", "PEQ", "BELL": return .peaking
        case "LSC", "LS", "LSH", "LS6", "LS12": return .lowShelf
        case "HSC", "HS", "HSH", "HS6", "HS12": return .highShelf
        case "LP", "LPQ", "LP6", "LP12": return .lowPass
        case "HP", "HPQ", "HP6", "HP12": return .highPass
        case "NO", "NOTCH": return .notch
        case "BP": return .bandPass
        default: return nil
        }
    }
}

public struct EqBand: Identifiable, Codable, Equatable {
    public var id = UUID()
    public var enabled: Bool
    public var filterType: EqFilterType
    public var freq: Double
    public var gainDb: Double
    public var q: Double

    public init(enabled: Bool = true, filterType: EqFilterType = .peaking, freq: Double = 1000.0, gainDb: Double = 0.0, q: Double = 1.41) {
        self.enabled = enabled
        self.filterType = filterType
        self.freq = freq
        self.gainDb = gainDb
        self.q = q
    }
}

public struct EqPreset: Identifiable, Codable, Equatable {
    public var id: String { name }
    public var name: String
    public var preampDb: Double
    public var bands: [EqBand]
}

public class EqManager: ObservableObject {
    public static let shared = EqManager()

    @Published public var isEnabled: Bool = false {
        didSet { saveAndApply() }
    }
    @Published public var presetName: String = "Flat (Mặc định)"
    @Published public var preampDb: Double = 0.0 {
        didSet { saveAndApply() }
    }
    @Published public var bands: [EqBand] = [] {
        didSet { saveAndApply() }
    }

    public let builtinPresets: [(name: String, text: String)] = [
        ("Flat (Mặc định)", """
        Preamp: 0.0 dB
        Filter 1: ON LSC Fc 105 Hz Gain 0.0 dB Q 0.70
        Filter 2: ON PK Fc 1000 Hz Gain 0.0 dB Q 1.41
        Filter 3: ON HSC Fc 10000 Hz Gain 0.0 dB Q 0.70
        """),
        ("AutoEQ: Sony WH-1000XM4 (Harman)", """
        Preamp: -6.3 dB
        Filter 1: ON LSC Fc 105 Hz Gain 3.1 dB Q 0.70
        Filter 2: ON PK Fc 205 Hz Gain -5.2 dB Q 1.15
        Filter 3: ON PK Fc 890 Hz Gain 2.4 dB Q 1.40
        Filter 4: ON PK Fc 2450 Hz Gain 4.8 dB Q 1.80
        Filter 5: ON PK Fc 4300 Hz Gain 3.9 dB Q 2.10
        Filter 6: ON HSC Fc 10000 Hz Gain 4.5 dB Q 0.70
        """),
        ("AutoEQ: Sennheiser HD600 (Harman)", """
        Preamp: -5.8 dB
        Filter 1: ON LSC Fc 105 Hz Gain 5.6 dB Q 0.70
        Filter 2: ON PK Fc 210 Hz Gain -1.8 dB Q 1.20
        Filter 3: ON PK Fc 3300 Hz Gain -2.1 dB Q 2.20
        Filter 4: ON HSC Fc 10000 Hz Gain 2.0 dB Q 0.70
        """),
        ("AutoEQ: AirPods Pro 2 (Harman)", """
        Preamp: -4.2 dB
        Filter 1: ON LSC Fc 105 Hz Gain 2.2 dB Q 0.70
        Filter 2: ON PK Fc 350 Hz Gain -1.6 dB Q 1.30
        Filter 3: ON PK Fc 1850 Hz Gain 3.1 dB Q 1.90
        Filter 4: ON PK Fc 5500 Hz Gain -2.4 dB Q 2.50
        Filter 5: ON HSC Fc 10500 Hz Gain 3.2 dB Q 0.70
        """),
        ("AutoEQ: Moondrop Aria / Blessing", """
        Preamp: -4.8 dB
        Filter 1: ON LSC Fc 105 Hz Gain 3.8 dB Q 0.70
        Filter 2: ON PK Fc 280 Hz Gain -1.4 dB Q 1.20
        Filter 3: ON PK Fc 3100 Hz Gain -2.2 dB Q 2.00
        Filter 4: ON PK Fc 6200 Hz Gain 2.5 dB Q 2.80
        Filter 5: ON HSC Fc 11000 Hz Gain 3.6 dB Q 0.70
        """),
        ("AutoEQ: Sony IER-M9 / XBA-N3", """
        Preamp: -5.0 dB
        Filter 1: ON LSC Fc 105 Hz Gain 2.5 dB Q 0.70
        Filter 2: ON PK Fc 230 Hz Gain -2.8 dB Q 1.10
        Filter 3: ON PK Fc 3000 Hz Gain 3.5 dB Q 1.70
        Filter 4: ON HSC Fc 10000 Hz Gain 4.0 dB Q 0.70
        """),
        ("Harman Bass Boost (+6.5dB)", """
        Preamp: -6.5 dB
        Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.71
        Filter 2: ON PK Fc 220 Hz Gain -1.5 dB Q 1.40
        Filter 3: ON PK Fc 3200 Hz Gain 2.0 dB Q 1.60
        Filter 4: ON HSC Fc 10000 Hz Gain 2.5 dB Q 0.70
        """),
        ("Audiophile Vocal & Air", """
        Preamp: -4.0 dB
        Filter 1: ON LSC Fc 90 Hz Gain 1.8 dB Q 0.70
        Filter 2: ON PK Fc 250 Hz Gain -1.5 dB Q 1.41
        Filter 3: ON PK Fc 1500 Hz Gain 2.2 dB Q 1.30
        Filter 4: ON PK Fc 3500 Hz Gain 2.8 dB Q 1.80
        Filter 5: ON HSC Fc 11000 Hz Gain 3.5 dB Q 0.70
        """)
    ]

    private init() {
        if let savedText = UserDefaults.standard.string(forKey: "peq_autoeq_text") {
            let name = UserDefaults.standard.string(forKey: "peq_preset_name") ?? "Custom AutoEQ"
            let en = UserDefaults.standard.bool(forKey: "peq_enabled")
            loadFromAutoEQText(name: name, text: savedText, enable: en)
        } else {
            loadFromAutoEQText(name: builtinPresets[0].name, text: builtinPresets[0].text, enable: false)
        }
    }

    public func loadFromAutoEQText(name: String, text: String, enable: Bool = true) {
        var parsedPreamp = 0.0
        var parsedBands: [EqBand] = []

        for rawLine in text.components(separatedBy: .newlines) {
            let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
            if line.isEmpty || line.hasPrefix("#") || line.hasPrefix(";") { continue }

            if line.lowercased().hasPrefix("preamp:") {
                let rest = line.dropFirst(7)
                    .replacingOccurrences(of: "dB", with: "", options: .caseInsensitive)
                    .trimmingCharacters(in: .whitespaces)
                if let v = Double(rest) {
                    parsedPreamp = min(max(v, -24.0), 12.0)
                }
                continue
            }

            if line.lowercased().hasPrefix("filter"), let colonIdx = line.firstIndex(of: ":") {
                let body = line[line.index(after: colonIdx)...].trimmingCharacters(in: .whitespaces)
                let tokens = body.components(separatedBy: .whitespaces).filter { !$0.isEmpty }
                guard !tokens.isEmpty else { continue }

                var i = 0
                var bandEnabled = true
                if tokens[i].uppercased() == "ON" {
                    bandEnabled = true
                    i += 1
                } else if tokens[i].uppercased() == "OFF" {
                    bandEnabled = false
                    i += 1
                }
                guard i < tokens.count, let fType = EqFilterType.fromToken(tokens[i]) else { continue }
                i += 1

                var freq = 1000.0
                var gain = 0.0
                var q = (fType == .lowShelf || fType == .highShelf) ? 0.707 : 1.41

                while i < tokens.count {
                    let key = tokens[i].uppercased()
                    if key == "FC" && i + 1 < tokens.count {
                        let clean = tokens[i + 1].replacingOccurrences(of: "Hz", with: "", options: .caseInsensitive)
                        if let v = Double(clean) { freq = min(max(v, 20.0), 20000.0) }
                        i += 2
                    } else if key == "GAIN" && i + 1 < tokens.count {
                        let clean = tokens[i + 1].replacingOccurrences(of: "dB", with: "", options: .caseInsensitive)
                        if let v = Double(clean) { gain = min(max(v, -24.0), 24.0) }
                        i += 2
                    } else if key == "Q" && i + 1 < tokens.count {
                        if let v = Double(tokens[i + 1]) { q = min(max(v, 0.1), 18.0) }
                        i += 2
                    } else {
                        i += 1
                    }
                }

                parsedBands.append(EqBand(enabled: bandEnabled, filterType: fType, freq: freq, gainDb: gain, q: q))
            }
        }

        self.presetName = name
        self.preampDb = parsedPreamp
        self.bands = parsedBands
        self.isEnabled = enable
        saveAndApply()
    }

    public func toAutoEQText() -> String {
        var out = String(format: "Preamp: %.1f dB\n", preampDb)
        for (i, b) in bands.enumerated() {
            let state = b.enabled ? "ON" : "OFF"
            out += String(format: "Filter %d: %@ %@ Fc %.0f Hz Gain %.1f dB Q %.2f\n",
                          i + 1, state, b.filterType.rawValue, b.freq, b.gainDb, b.q)
        }
        return out
    }

    public func responseDb(at freqHz: Double, sampleRate: Double = 48000.0) -> Double {
        var total = preampDb
        for b in bands where b.enabled {
            total += Self.bandMagnitudeDb(band: b, freqHz: freqHz, sampleRate: sampleRate)
        }
        return total
    }

    private static func bandMagnitudeDb(band: EqBand, freqHz: Double, sampleRate: Double) -> Double {
        let fs = max(sampleRate, 8000.0)
        let f0 = min(max(band.freq, 10.0), fs * 0.49)
        let q = min(max(band.q, 0.1), 18.0)
        let db = min(max(band.gainDb, -24.0), 24.0)

        let a = pow(10.0, db / 40.0)
        let w0 = 2.0 * Double.pi * f0 / fs
        let cosW0 = cos(w0)
        let sinW0 = sin(w0)
        let alpha = sinW0 / (2.0 * q)

        var b0 = 1.0, b1 = 0.0, b2 = 0.0, a0 = 1.0, a1 = 0.0, a2 = 0.0
        switch band.filterType {
        case .peaking:
            b0 = 1.0 + alpha * a
            b1 = -2.0 * cosW0
            b2 = 1.0 - alpha * a
            a0 = 1.0 + alpha / a
            a1 = -2.0 * cosW0
            a2 = 1.0 - alpha / a
        case .lowShelf:
            let s = 2.0 * sqrt(a) * alpha
            b0 = a * ((a + 1.0) - (a - 1.0) * cosW0 + s)
            b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cosW0)
            b2 = a * ((a + 1.0) - (a - 1.0) * cosW0 - s)
            a0 = (a + 1.0) + (a - 1.0) * cosW0 + s
            a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cosW0)
            a2 = (a + 1.0) + (a - 1.0) * cosW0 - s
        case .highShelf:
            let s = 2.0 * sqrt(a) * alpha
            b0 = a * ((a + 1.0) + (a - 1.0) * cosW0 + s)
            b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cosW0)
            b2 = a * ((a + 1.0) + (a - 1.0) * cosW0 - s)
            a0 = (a + 1.0) - (a - 1.0) * cosW0 + s
            a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cosW0)
            a2 = (a + 1.0) - (a - 1.0) * cosW0 - s
        case .lowPass:
            b0 = (1.0 - cosW0) * 0.5
            b1 = 1.0 - cosW0
            b2 = (1.0 - cosW0) * 0.5
            a0 = 1.0 + alpha
            a1 = -2.0 * cosW0
            a2 = 1.0 - alpha
        case .highPass:
            b0 = (1.0 + cosW0) * 0.5
            b1 = -(1.0 + cosW0)
            b2 = (1.0 + cosW0) * 0.5
            a0 = 1.0 + alpha
            a1 = -2.0 * cosW0
            a2 = 1.0 - alpha
        case .notch:
            b0 = 1.0
            b1 = -2.0 * cosW0
            b2 = 1.0
            a0 = 1.0 + alpha
            a1 = -2.0 * cosW0
            a2 = 1.0 - alpha
        case .bandPass:
            b0 = alpha
            b1 = 0.0
            b2 = -alpha
            a0 = 1.0 + alpha
            a1 = -2.0 * cosW0
            a2 = 1.0 - alpha
        }

        b0 /= a0; b1 /= a0; b2 /= a0; a1 /= a0; a2 /= a0
        let w = 2.0 * Double.pi * freqHz / fs
        let cw = cos(w), sw = sin(w), c2w = cos(2.0 * w), s2w = sin(2.0 * w)
        let numR = b0 + b1 * cw + b2 * c2w
        let numI = -(b1 * sw + b2 * s2w)
        let denR = 1.0 + a1 * cw + a2 * c2w
        let denI = -(a1 * sw + a2 * s2w)
        let numMag2 = numR * numR + numI * numI
        let denMag2 = max(denR * denR + denI * denI, 1e-18)
        return 10.0 * log10(numMag2 / denMag2)
    }

    public var linearGainMultiplier: Float {
        guard isEnabled else { return 1.0 }
        let linear = pow(10.0, preampDb / 20.0)
        return Float(min(max(linear, 0.05), 1.0))
    }

    private func saveAndApply() {
        UserDefaults.standard.set(isEnabled, forKey: "peq_enabled")
        UserDefaults.standard.set(presetName, forKey: "peq_preset_name")
        UserDefaults.standard.set(toAutoEQText(), forKey: "peq_autoeq_text")
        AudioPlayerManager.shared.applyEqSettings()
    }
}
