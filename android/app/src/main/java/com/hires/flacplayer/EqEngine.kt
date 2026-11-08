package com.hires.flacplayer

import kotlin.math.*

enum class FilterType(val code: String, val label: String) {
    PEAKING("PK", "PK (Peaking)"),
    LOW_SHELF("LSC", "LSC (Low Shelf)"),
    HIGH_SHELF("HSC", "HSC (High Shelf)"),
    LOW_PASS("LPQ", "LP (Low Pass)"),
    HIGH_PASS("HPQ", "HP (High Pass)"),
    NOTCH("NO", "NO (Notch)"),
    BAND_PASS("BP", "BP (Band Pass)");

    companion object {
        fun fromToken(token: String): FilterType? {
            return when (token.uppercase().trim()) {
                "PK", "PEA", "PEQ", "BELL" -> PEAKING
                "LSC", "LS", "LSH", "LS6", "LS12" -> LOW_SHELF
                "HSC", "HS", "HSH", "HS6", "HS12" -> HIGH_SHELF
                "LP", "LPQ", "LP6", "LP12" -> LOW_PASS
                "HP", "HPQ", "HP6", "HP12" -> HIGH_PASS
                "NO", "NOTCH" -> NOTCH
                "BP" -> BAND_PASS
                else -> null
            }
        }
    }
}

data class EqBand(
    var enabled: Boolean = true,
    var filterType: FilterType = FilterType.PEAKING,
    var freq: Double = 1000.0,
    var gainDb: Double = 0.0,
    var q: Double = 1.41
)

data class EqConfig(
    var enabled: Boolean = false,
    var name: String = "Flat (Mặc định)",
    var preampDb: Double = 0.0,
    var bands: MutableList<EqBand> = mutableListOf()
) {
    fun toAutoEqText(): String {
        val sb = StringBuilder()
        sb.append(String.format(java.util.Locale.US, "Preamp: %.1f dB\n", preampDb))
        bands.forEachIndexed { i, b ->
            val state = if (b.enabled) "ON" else "OFF"
            sb.append(
                String.format(
                    java.util.Locale.US,
                    "Filter %d: %s %s Fc %.0f Hz Gain %.1f dB Q %.2f\n",
                    i + 1, state, b.filterType.code, b.freq, b.gainDb, b.q
                )
            )
        }
        return sb.toString()
    }

    fun responseDb(freqHz: Double, sampleRate: Double = 48000.0): Double {
        var total = preampDb
        for (b in bands) {
            if (b.enabled) {
                total += BiquadCoeffs.design(b, sampleRate).magnitudeDb(freqHz, sampleRate)
            }
        }
        return total
    }

    fun autoPreamp(): Double {
        var maxGain = 0.0
        val copy = this.copy(preampDb = 0.0)
        for (i in 0 until 120) {
            val f = 20.0 * 1000.0.pow(i.toDouble() / 119.0)
            val g = copy.responseDb(f, 48000.0)
            if (g > maxGain) maxGain = g
        }
        return (-maxGain - 0.2).coerceAtMost(0.0).coerceIn(-24.0, 12.0)
    }

    companion object {
        fun fromAutoEqText(name: String, text: String, enable: Boolean = true): EqConfig {
            var preamp = 0.0
            val bands = mutableListOf<EqBand>()

            for (raw in text.lines()) {
                val line = raw.trim()
                if (line.isEmpty() || line.startsWith("#") || line.startsWith(";")) continue
                val lower = line.lowercase()
                if (lower.startsWith("preamp:")) {
                    val vStr = line.substring(7).replace("dB", "", ignoreCase = true).trim().replace(",", ".")
                    vStr.toDoubleOrNull()?.let { preamp = it.coerceIn(-24.0, 12.0) }
                    continue
                }
                if (lower.startsWith("filter") && line.contains(":")) {
                    val body = line.substringAfter(":").trim()
                    val tokens = body.split(Regex("\\s+")).filter { it.isNotEmpty() }
                    if (tokens.isEmpty()) continue
                    var idx = 0
                    var bandOn = true
                    if (tokens[idx].equals("ON", ignoreCase = true)) {
                        bandOn = true
                        idx++
                    } else if (tokens[idx].equals("OFF", ignoreCase = true)) {
                        bandOn = false
                        idx++
                    }
                    if (idx >= tokens.size) continue
                    val ft = FilterType.fromToken(tokens[idx]) ?: continue
                    idx++

                    var freq = 1000.0
                    var gain = 0.0
                    var q = if (ft == FilterType.LOW_SHELF || ft == FilterType.HIGH_SHELF) 0.707 else 1.41

                    while (idx < tokens.size) {
                        val key = tokens[idx].uppercase()
                        if (key == "FC" && idx + 1 < tokens.size) {
                            val clean = tokens[idx + 1].replace("Hz", "", ignoreCase = true).replace(",", ".")
                            clean.toDoubleOrNull()?.let { freq = it.coerceIn(20.0, 20000.0) }
                            idx += 2
                            if (idx < tokens.size && tokens[idx].equals("Hz", ignoreCase = true)) idx++
                        } else if (key == "GAIN" && idx + 1 < tokens.size) {
                            val clean = tokens[idx + 1].replace("dB", "", ignoreCase = true).replace(",", ".")
                            clean.toDoubleOrNull()?.let { gain = it.coerceIn(-24.0, 24.0) }
                            idx += 2
                            if (idx < tokens.size && tokens[idx].equals("dB", ignoreCase = true)) idx++
                        } else if (key == "Q" && idx + 1 < tokens.size) {
                            val clean = tokens[idx + 1].replace(",", ".")
                            clean.toDoubleOrNull()?.let { q = it.coerceIn(0.1, 18.0) }
                            idx += 2
                        } else {
                            idx++
                        }
                    }
                    bands.add(EqBand(bandOn, ft, freq, gain, q))
                }
            }

            if (bands.isEmpty()) {
                bands.add(EqBand(true, FilterType.LOW_SHELF, 105.0, 0.0, 0.70))
                bands.add(EqBand(true, FilterType.PEAKING, 1000.0, 0.0, 1.41))
                bands.add(EqBand(true, FilterType.HIGH_SHELF, 10000.0, 0.0, 0.70))
            }
            return EqConfig(enable, name, preamp, bands)
        }

        val BUILTIN_PRESETS: List<Pair<String, String>> = listOf(
            "Flat (Mặc định)" to """
                Preamp: 0.0 dB
                Filter 1: ON LSC Fc 105 Hz Gain 0.0 dB Q 0.70
                Filter 2: ON PK Fc 250 Hz Gain 0.0 dB Q 1.41
                Filter 3: ON PK Fc 1000 Hz Gain 0.0 dB Q 1.41
                Filter 4: ON PK Fc 3500 Hz Gain 0.0 dB Q 1.41
                Filter 5: ON HSC Fc 10000 Hz Gain 0.0 dB Q 0.70
            """.trimIndent(),
            "AutoEQ: Sony WH-1000XM4 (Harman)" to """
                Preamp: -6.3 dB
                Filter 1: ON LSC Fc 105 Hz Gain 3.1 dB Q 0.70
                Filter 2: ON PK Fc 205 Hz Gain -5.2 dB Q 1.15
                Filter 3: ON PK Fc 890 Hz Gain 2.4 dB Q 1.40
                Filter 4: ON PK Fc 2450 Hz Gain 4.8 dB Q 1.80
                Filter 5: ON PK Fc 4300 Hz Gain 3.9 dB Q 2.10
                Filter 6: ON HSC Fc 10000 Hz Gain 4.5 dB Q 0.70
            """.trimIndent(),
            "AutoEQ: Sony IER-M9 / XBA-N3" to """
                Preamp: -5.0 dB
                Filter 1: ON LSC Fc 105 Hz Gain 2.5 dB Q 0.70
                Filter 2: ON PK Fc 230 Hz Gain -2.8 dB Q 1.10
                Filter 3: ON PK Fc 3000 Hz Gain 3.5 dB Q 1.70
                Filter 4: ON HSC Fc 10000 Hz Gain 4.0 dB Q 0.70
            """.trimIndent(),
            "AutoEQ: Sennheiser HD600 (Harman)" to """
                Preamp: -5.8 dB
                Filter 1: ON LSC Fc 105 Hz Gain 5.6 dB Q 0.70
                Filter 2: ON PK Fc 210 Hz Gain -1.8 dB Q 1.20
                Filter 3: ON PK Fc 3300 Hz Gain -2.1 dB Q 2.20
                Filter 4: ON HSC Fc 10000 Hz Gain 2.0 dB Q 0.70
            """.trimIndent(),
            "AutoEQ: Moondrop Aria / Blessing" to """
                Preamp: -4.8 dB
                Filter 1: ON LSC Fc 105 Hz Gain 3.8 dB Q 0.70
                Filter 2: ON PK Fc 280 Hz Gain -1.4 dB Q 1.20
                Filter 3: ON PK Fc 3100 Hz Gain -2.2 dB Q 2.00
                Filter 4: ON PK Fc 6200 Hz Gain 2.5 dB Q 2.80
                Filter 5: ON HSC Fc 11000 Hz Gain 3.6 dB Q 0.70
            """.trimIndent(),
            "AutoEQ: AirPods Pro 2 (Harman)" to """
                Preamp: -4.2 dB
                Filter 1: ON LSC Fc 105 Hz Gain 2.2 dB Q 0.70
                Filter 2: ON PK Fc 350 Hz Gain -1.6 dB Q 1.30
                Filter 3: ON PK Fc 1850 Hz Gain 3.1 dB Q 1.90
                Filter 4: ON PK Fc 5500 Hz Gain -2.4 dB Q 2.50
                Filter 5: ON HSC Fc 10500 Hz Gain 3.2 dB Q 0.70
            """.trimIndent(),
            "Harman Bass Boost (+6.5dB Sub-Bass)" to """
                Preamp: -6.5 dB
                Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.71
                Filter 2: ON PK Fc 220 Hz Gain -1.5 dB Q 1.40
                Filter 3: ON PK Fc 3200 Hz Gain 2.0 dB Q 1.60
                Filter 4: ON HSC Fc 10000 Hz Gain 2.5 dB Q 0.70
            """.trimIndent(),
            "Audiophile Vocal & Air (Chi tiết)" to """
                Preamp: -4.0 dB
                Filter 1: ON LSC Fc 90 Hz Gain 1.8 dB Q 0.70
                Filter 2: ON PK Fc 250 Hz Gain -1.5 dB Q 1.41
                Filter 3: ON PK Fc 1500 Hz Gain 2.2 dB Q 1.30
                Filter 4: ON PK Fc 3500 Hz Gain 2.8 dB Q 1.80
                Filter 5: ON HSC Fc 11000 Hz Gain 3.5 dB Q 0.70
            """.trimIndent()
        )
    }
}

class BiquadCoeffs(
    val b0: Double,
    val b1: Double,
    val b2: Double,
    val a1: Double,
    val a2: Double
) {
    fun magnitudeDb(freqHz: Double, sampleRate: Double): Double {
        val fs = sampleRate.coerceAtLeast(8000.0)
        val w = 2.0 * PI * freqHz / fs
        val cw = cos(w)
        val sw = sin(w)
        val c2w = cos(2.0 * w)
        val s2w = sin(2.0 * w)

        val numR = b0 + b1 * cw + b2 * c2w
        val numI = -(b1 * sw + b2 * s2w)
        val denR = 1.0 + a1 * cw + a2 * c2w
        val denI = -(a1 * sw + a2 * s2w)

        val numMag2 = numR * numR + numI * numI
        val denMag2 = (denR * denR + denI * denI).coerceAtLeast(1e-18)
        return 10.0 * log10(numMag2 / denMag2)
    }

    companion object {
        fun design(band: EqBand, sampleRate: Double): BiquadCoeffs {
            val fs = sampleRate.coerceAtLeast(8000.0)
            val f0 = band.freq.coerceIn(10.0, fs * 0.49)
            val q = band.q.coerceIn(0.1, 18.0)
            val db = band.gainDb.coerceIn(-24.0, 24.0)

            val a = 10.0.pow(db / 40.0)
            val w0 = 2.0 * PI * f0 / fs
            val cosW0 = cos(w0)
            val sinW0 = sin(w0)
            val alpha = sinW0 / (2.0 * q)

            val b0: Double
            val b1: Double
            val b2: Double
            val a0: Double
            val a1: Double
            val a2: Double

            when (band.filterType) {
                FilterType.PEAKING -> {
                    b0 = 1.0 + alpha * a
                    b1 = -2.0 * cosW0
                    b2 = 1.0 - alpha * a
                    a0 = 1.0 + alpha / a
                    a1 = -2.0 * cosW0
                    a2 = 1.0 - alpha / a
                }
                FilterType.LOW_SHELF -> {
                    val s = 2.0 * sqrt(a) * alpha
                    b0 = a * ((a + 1.0) - (a - 1.0) * cosW0 + s)
                    b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cosW0)
                    b2 = a * ((a + 1.0) - (a - 1.0) * cosW0 - s)
                    a0 = (a + 1.0) + (a - 1.0) * cosW0 + s
                    a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cosW0)
                    a2 = (a + 1.0) + (a - 1.0) * cosW0 - s
                }
                FilterType.HIGH_SHELF -> {
                    val s = 2.0 * sqrt(a) * alpha
                    b0 = a * ((a + 1.0) + (a - 1.0) * cosW0 + s)
                    b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cosW0)
                    b2 = a * ((a + 1.0) + (a - 1.0) * cosW0 - s)
                    a0 = (a + 1.0) - (a - 1.0) * cosW0 + s
                    a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cosW0)
                    a2 = (a + 1.0) - (a - 1.0) * cosW0 - s
                }
                FilterType.LOW_PASS -> {
                    b0 = (1.0 - cosW0) * 0.5
                    b1 = 1.0 - cosW0
                    b2 = (1.0 - cosW0) * 0.5
                    a0 = 1.0 + alpha
                    a1 = -2.0 * cosW0
                    a2 = 1.0 - alpha
                }
                FilterType.HIGH_PASS -> {
                    b0 = (1.0 + cosW0) * 0.5
                    b1 = -(1.0 + cosW0)
                    b2 = (1.0 + cosW0) * 0.5
                    a0 = 1.0 + alpha
                    a1 = -2.0 * cosW0
                    a2 = 1.0 - alpha
                }
                FilterType.NOTCH -> {
                    b0 = 1.0
                    b1 = -2.0 * cosW0
                    b2 = 1.0
                    a0 = 1.0 + alpha
                    a1 = -2.0 * cosW0
                    a2 = 1.0 - alpha
                }
                FilterType.BAND_PASS -> {
                    b0 = alpha
                    b1 = 0.0
                    b2 = -alpha
                    a0 = 1.0 + alpha
                    a1 = -2.0 * cosW0
                    a2 = 1.0 - alpha
                }
            }
            return BiquadCoeffs(b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
        }
    }
}

class BiquadProcessor {
    @Volatile
    private var config: EqConfig = EqConfig()
    private var sampleRate: Int = 44100
    private var channels: Int = 2
    private var preampGain: Double = 1.0
    private var coeffs: Array<BiquadCoeffs> = emptyArray()
    private var z1: Array<DoubleArray> = emptyArray()
    private var z2: Array<DoubleArray> = emptyArray()

    @Synchronized
    fun updateConfig(newConfig: EqConfig) {
        val reset = config.bands.size != newConfig.bands.size
        config = newConfig
        rebuild(reset)
    }

    @Synchronized
    fun setFormat(rate: Int, ch: Int) {
        val r = rate.coerceAtLeast(8000)
        val c = ch.coerceAtLeast(1)
        if (r != sampleRate || c != channels) {
            sampleRate = r
            channels = c
            rebuild(true)
        }
    }

    @Synchronized
    fun resetState() {
        for (b in z1.indices) {
            z1[b].fill(0.0)
            z2[b].fill(0.0)
        }
    }

    @Synchronized
    private fun rebuild(resetState: Boolean) {
        preampGain = 10.0.pow(config.preampDb / 20.0)
        val active = config.bands.filter { it.enabled }.map { BiquadCoeffs.design(it, sampleRate.toDouble()) }
        coeffs = active.toTypedArray()
        if (resetState || z1.size != coeffs.size || (z1.isNotEmpty() && z1[0].size != channels)) {
            z1 = Array(coeffs.size) { DoubleArray(channels) }
            z2 = Array(coeffs.size) { DoubleArray(channels) }
        }
    }

    @Synchronized
    fun processPcm16(pcmBytes: ByteArray, length: Int, outFloat: FloatArray): Int {
        val sampleCount = length / 2
        val enabled = config.enabled && coeffs.isNotEmpty()
        val chCount = channels.coerceAtLeast(1)
        val pre = preampGain
        val cf = coeffs

        var sIdx = 0
        var bIdx = 0
        while (sIdx < sampleCount) {
            val lo = pcmBytes[bIdx].toInt() and 0xFF
            val hi = pcmBytes[bIdx + 1].toInt()
            val s16 = (hi shl 8) or lo
            var x = s16.toDouble() / 32768.0

            if (enabled) {
                val ch = sIdx % chCount
                x *= pre
                for (f in cf.indices) {
                    val c = cf[f]
                    val y = c.b0 * x + z1[f][ch]
                    z1[f][ch] = c.b1 * x - c.a1 * y + z2[f][ch]
                    z2[f][ch] = c.b2 * x - c.a2 * y
                    x = y
                }
                if (x > 0.98) {
                    x = 0.98 + 0.02 * tanh((x - 0.98) / 0.02)
                } else if (x < -0.98) {
                    x = -0.98 - 0.02 * tanh((-x - 0.98) / 0.02)
                }
            }
            outFloat[sIdx] = x.toFloat()
            sIdx++
            bIdx += 2
        }
        return sampleCount
    }
}
