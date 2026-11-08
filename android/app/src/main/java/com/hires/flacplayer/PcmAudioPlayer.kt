package com.hires.flacplayer

import android.media.*
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

class PcmAudioPlayer(
    private val onProgress: (Double, Double) -> Unit,
    private val onTrackEnded: () -> Unit
) {
    val eqProcessor = BiquadProcessor()

    @Volatile
    var isPlaying: Boolean = false
        private set

    private var decodeThread: Thread? = null
    private val stopFlag = AtomicBoolean(false)
    private val pauseFlag = AtomicBoolean(false)
    private val seekTargetUs = AtomicLong(-1L)

    fun play(pathOrUrl: String) {
        stop()
        stopFlag.set(false)
        pauseFlag.set(false)
        seekTargetUs.set(-1L)
        isPlaying = true

        decodeThread = Thread {
            runDecodeLoop(pathOrUrl)
        }.apply {
            name = "HiRes-FLAC-DSP"
            priority = Thread.MAX_PRIORITY
            start()
        }
    }

    fun togglePause(): Boolean {
        val nextPaused = !pauseFlag.get()
        pauseFlag.set(nextPaused)
        isPlaying = !nextPaused
        return isPlaying
    }

    fun seekToSeconds(sec: Double) {
        seekTargetUs.set((sec * 1_000_000.0).toLong().coerceAtLeast(0L))
    }

    fun stop() {
        stopFlag.set(true)
        pauseFlag.set(false)
        isPlaying = false
        decodeThread?.let {
            try {
                it.join(600)
            } catch (_: Exception) {}
        }
        decodeThread = null
    }

    private fun runDecodeLoop(pathOrUrl: String) {
        val extractor = MediaExtractor()
        var codec: MediaCodec? = null
        var audioTrack: AudioTrack? = null

        try {
            extractor.setDataSource(pathOrUrl)
            var audioTrackIdx = -1
            var format: MediaFormat? = null
            for (i in 0 until extractor.trackCount) {
                val f = extractor.getTrackFormat(i)
                val mime = f.getString(MediaFormat.KEY_MIME) ?: ""
                if (mime.startsWith("audio/")) {
                    audioTrackIdx = i
                    format = f
                    break
                }
            }
            if (audioTrackIdx < 0 || format == null) return

            extractor.selectTrack(audioTrackIdx)
            val mime = format.getString(MediaFormat.KEY_MIME) ?: return
            var sampleRate = if (format.containsKey(MediaFormat.KEY_SAMPLE_RATE)) format.getInteger(MediaFormat.KEY_SAMPLE_RATE) else 44100
            var channels = if (format.containsKey(MediaFormat.KEY_CHANNEL_COUNT)) format.getInteger(MediaFormat.KEY_CHANNEL_COUNT) else 2
            val durationUs = if (format.containsKey(MediaFormat.KEY_DURATION)) format.getLong(MediaFormat.KEY_DURATION) else 0L
            val durationSec = durationUs.toDouble() / 1_000_000.0

            codec = MediaCodec.createDecoderByType(mime)
            codec.configure(format, null, null, 0)
            codec.start()

            eqProcessor.setFormat(sampleRate, channels)
            eqProcessor.resetState()

            fun buildAudioTrack(sr: Int, ch: Int): AudioTrack {
                val chOut = if (ch == 1) AudioFormat.CHANNEL_OUT_MONO else AudioFormat.CHANNEL_OUT_STEREO
                val minBuf = AudioTrack.getMinBufferSize(sr, chOut, AudioFormat.ENCODING_PCM_FLOAT).coerceAtLeast(8192)
                return AudioTrack(
                    AudioManager.STREAM_MUSIC,
                    sr,
                    chOut,
                    AudioFormat.ENCODING_PCM_FLOAT,
                    minBuf * 2,
                    AudioTrack.MODE_STREAM
                ).apply { play() }
            }

            audioTrack = buildAudioTrack(sampleRate, channels)

            val info = MediaCodec.BufferInfo()
            var inputDone = false
            var outputDone = false
            var pcmBytes = ByteArray(65536)
            var floatOut = FloatArray(32768)

            while (!stopFlag.get() && !outputDone) {
                if (pauseFlag.get()) {
                    Thread.sleep(30)
                    continue
                }

                val seekUs = seekTargetUs.getAndSet(-1L)
                if (seekUs >= 0L) {
                    extractor.seekTo(seekUs, MediaExtractor.SEEK_TO_CLOSEST_SYNC)
                    codec.flush()
                    eqProcessor.resetState()
                    inputDone = false
                }

                if (!inputDone) {
                    val inIdx = codec.dequeueInputBuffer(10_000)
                    if (inIdx >= 0) {
                        val inBuf = codec.getInputBuffer(inIdx)
                        if (inBuf != null) {
                            val read = extractor.readSampleData(inBuf, 0)
                            if (read < 0) {
                                codec.queueInputBuffer(inIdx, 0, 0, 0L, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                                inputDone = true
                            } else {
                                val pts = extractor.sampleTime
                                codec.queueInputBuffer(inIdx, 0, read, pts, 0)
                                extractor.advance()
                            }
                        }
                    }
                }

                val outIdx = codec.dequeueOutputBuffer(info, 10_000)
                when {
                    outIdx >= 0 -> {
                        val outBuf = codec.getOutputBuffer(outIdx)
                        if (outBuf != null && info.size > 0) {
                            outBuf.position(info.offset)
                            outBuf.limit(info.offset + info.size)
                            if (pcmBytes.size < info.size) {
                                pcmBytes = ByteArray(info.size)
                                floatOut = FloatArray(info.size / 2 + 16)
                            }
                            outBuf.get(pcmBytes, 0, info.size)

                            val samples = eqProcessor.processPcm16(pcmBytes, info.size, floatOut)
                            audioTrack?.write(floatOut, 0, samples, AudioTrack.WRITE_BLOCKING)

                            val posSec = info.presentationTimeUs.toDouble() / 1_000_000.0
                            onProgress(posSec, durationSec)
                        }
                        codec.releaseOutputBuffer(outIdx, false)
                        if ((info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM) != 0) {
                            outputDone = true
                        }
                    }
                    outIdx == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED -> {
                        val newFmt = codec.outputFormat
                        val newSr = if (newFmt.containsKey(MediaFormat.KEY_SAMPLE_RATE)) newFmt.getInteger(MediaFormat.KEY_SAMPLE_RATE) else sampleRate
                        val newCh = if (newFmt.containsKey(MediaFormat.KEY_CHANNEL_COUNT)) newFmt.getInteger(MediaFormat.KEY_CHANNEL_COUNT) else channels
                        if (newSr != sampleRate || newCh != channels) {
                            sampleRate = newSr
                            channels = newCh
                            eqProcessor.setFormat(sampleRate, channels)
                            audioTrack?.stop()
                            audioTrack?.release()
                            audioTrack = buildAudioTrack(sampleRate, channels)
                        }
                    }
                }
            }

            if (!stopFlag.get() && outputDone) {
                isPlaying = false
                onTrackEnded()
            }
        } catch (_: Exception) {
            isPlaying = false
        } finally {
            try { audioTrack?.stop() } catch (_: Exception) {}
            try { audioTrack?.release() } catch (_: Exception) {}
            try { codec?.stop() } catch (_: Exception) {}
            try { codec?.release() } catch (_: Exception) {}
            try { extractor.release() } catch (_: Exception) {}
        }
    }
}
