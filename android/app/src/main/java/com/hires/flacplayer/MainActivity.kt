package com.hires.flacplayer

import android.Manifest
import android.app.AlertDialog
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.database.ContentObserver
import android.graphics.*
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.text.Editable
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.*
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import org.json.JSONArray
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import kotlin.math.pow

data class AudioItem(
    val title: String,
    val artist: String,
    val pathOrUri: String,
    val formatBadge: String,
    val durationSec: Double,
    val isOfflineSaved: Boolean = true
)

class MainActivity : AppCompatActivity() {

    private val uiHandler = Handler(Looper.getMainLooper())
    private lateinit var pcmPlayer: PcmAudioPlayer
    private var eqConfig: EqConfig = EqConfig()

    private val allTracks = mutableListOf<AudioItem>()
    private val filteredTracks = mutableListOf<AudioItem>()
    private var currentIndex = -1

    private lateinit var statusBanner: TextView
    private lateinit var eqButton: Button
    private lateinit var searchInput: EditText
    private lateinit var listView: ListView
    private lateinit var listAdapter: BaseAdapter
    private lateinit var nowPlayingTitle: TextView
    private lateinit var nowPlayingTime: TextView
    private lateinit var seekBar: SeekBar
    private lateinit var playPauseBtn: Button
    private var isUserSeeking = false

    private var pcServerUrl: String = "http://192.168.1.10:8080"
    private val knownTrackKeys = HashSet<String>()
    private var mediaObserver: ContentObserver? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val prefs = getSharedPreferences("flac_xz1_prefs", Context.MODE_PRIVATE)
        pcServerUrl = prefs.getString("pc_url", "http://192.168.1.10:8080") ?: "http://192.168.1.10:8080"
        val savedEqText = prefs.getString("peq_text", null)
        val savedEqName = prefs.getString("peq_name", "Flat (Mặc định)") ?: "Flat (Mặc định)"
        val savedEqOn = prefs.getBoolean("peq_enabled", false)
        if (savedEqText != null) {
            eqConfig = EqConfig.fromAutoEqText(savedEqName, savedEqText, savedEqOn)
        } else {
            val def = EqConfig.BUILTIN_PRESETS[0]
            eqConfig = EqConfig.fromAutoEqText(def.first, def.second, false)
        }

        pcmPlayer = PcmAudioPlayer(
            context = applicationContext,
            onProgress = { pos, dur ->
                uiHandler.post {
                    if (!isUserSeeking && dur > 0) {
                        seekBar.max = (dur * 1000).toInt()
                        seekBar.progress = (pos * 1000).toInt()
                        nowPlayingTime.text = "${fmtTime(pos)} / ${fmtTime(dur)}"
                    }
                }
            },
            onTrackEnded = {
                uiHandler.post { playNext() }
            }
        )
        pcmPlayer.eqProcessor.updateConfig(eqConfig)

        buildDarkHiResUI()
        registerMediaStoreWatcher()
        requestAudioPermissionAndScan()
        startAutoFolderAndServerWatcher()
    }

    private fun buildDarkHiResUI() {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.parseColor("#0D0E12"))
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        }

        // Top Bar
        val topBar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setBackgroundColor(Color.parseColor("#161820"))
            setPadding(32, 24, 32, 24)
        }
        val titleTv = TextView(this).apply {
            text = "◉ Hi-Res FLAC (XZ1 A15)"
            textSize = 17f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#4CC2FF"))
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
        }
        eqButton = Button(this).apply {
            text = if (eqConfig.enabled) "🎚 EQ: ON" else "🎚 AutoEQ"
            setTextColor(Color.parseColor("#F5C542"))
            setBackgroundColor(Color.parseColor("#232733"))
            setOnClickListener { showParametricEqDialog() }
        }
        val pcBtn = Button(this).apply {
            text = "💻 PC Link"
            setTextColor(Color.parseColor("#3DD68C"))
            setBackgroundColor(Color.parseColor("#232733"))
            setOnClickListener { showPcLinkDialog() }
        }
        topBar.addView(titleTv)
        topBar.addView(eqButton)
        topBar.addView(Space(this).apply { minimumWidth = 16 })
        topBar.addView(pcBtn)
        root.addView(topBar)

        // Status Banner
        statusBanner = TextView(this).apply {
            text = "🔄 Tự động quét nhạc trên Sony XZ1 (Android 15) & đồng bộ Offline từ PC..."
            textSize = 12f
            setTextColor(Color.parseColor("#3DD68C"))
            setBackgroundColor(Color.parseColor("#18221F"))
            setPadding(32, 14, 32, 14)
        }
        root.addView(statusBanner)

        // Search Box
        searchInput = EditText(this).apply {
            hint = "🔍 Tìm bài hát, nghệ sĩ, FLAC..."
            setHintTextColor(Color.parseColor("#6B7280"))
            setTextColor(Color.WHITE)
            setBackgroundColor(Color.parseColor("#1C202B"))
            setPadding(32, 20, 32, 20)
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) { filterPlaylist() }
                override fun afterTextChanged(s: Editable?) {}
            })
        }
        root.addView(searchInput)

        // Track ListView
        listView = ListView(this).apply {
            dividerHeight = 1
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)
        }
        listAdapter = object : BaseAdapter() {
            override fun getCount(): Int = filteredTracks.size
            override fun getItem(position: Int): Any = filteredTracks[position]
            override fun getItemId(position: Int): Long = position.toLong()
            override fun getView(position: Int, convertView: View?, parent: ViewGroup?): View {
                val item = filteredTracks[position]
                val row = (convertView as? LinearLayout) ?: LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    setPadding(36, 22, 36, 22)
                }
                row.removeAllViews()
                val active = (currentIndex in allTracks.indices && allTracks[currentIndex].pathOrUri == item.pathOrUri)
                row.setBackgroundColor(if (active) Color.parseColor("#18283A") else Color.TRANSPARENT)

                val t1 = TextView(this@MainActivity).apply {
                    text = "${position + 1}. ${item.title}"
                    textSize = 15f
                    setTypeface(typeface, if (active) Typeface.BOLD else Typeface.NORMAL)
                    setTextColor(if (active) Color.parseColor("#4CC2FF") else Color.WHITE)
                }
                val t2 = TextView(this@MainActivity).apply {
                    val savedTag = if (item.isOfflineSaved) " • ✔ Offline" else ""
                    text = "${item.artist} • [${item.formatBadge}]$savedTag"
                    textSize = 12f
                    setTextColor(Color.parseColor("#9AA0A6"))
                }
                row.addView(t1)
                row.addView(t2)
                return row
            }
        }
        listView.adapter = listAdapter
        listView.setOnItemClickListener { _, _, pos, _ ->
            val chosen = filteredTracks[pos]
            val idx = allTracks.indexOfFirst { it.pathOrUri == chosen.pathOrUri }
            if (idx >= 0) playIndex(idx)
        }
        root.addView(listView)

        // Bottom Player Bar
        val bottomBar = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.parseColor("#181B24"))
            setPadding(32, 20, 32, 28)
        }
        nowPlayingTitle = TextView(this).apply {
            text = "Chưa phát bài nào"
            textSize = 14f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.WHITE)
        }
        nowPlayingTime = TextView(this).apply {
            text = "0:00 / 0:00"
            textSize = 12f
            setTextColor(Color.parseColor("#9AA0A6"))
        }
        seekBar = SeekBar(this).apply {
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(sb: SeekBar?, progress: Int, fromUser: Boolean) {}
                override fun onStartTrackingTouch(sb: SeekBar?) { isUserSeeking = true }
                override fun onStopTrackingTouch(sb: SeekBar?) {
                    isUserSeeking = false
                    sb?.let { pcmPlayer.seekToSeconds(it.progress.toDouble() / 1000.0) }
                }
            })
        }

        val ctrlRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        val prevBtn = Button(this).apply {
            text = "⏮ Trước"
            setOnClickListener { playPrev() }
        }
        playPauseBtn = Button(this).apply {
            text = "▶ Phát"
            setBackgroundColor(Color.parseColor("#4CC2FF"))
            setTextColor(Color.BLACK)
            setOnClickListener {
                if (currentIndex == -1 && allTracks.isNotEmpty()) {
                    playIndex(0)
                } else {
                    val playing = pcmPlayer.togglePause()
                    text = if (playing) "⏸ Tạm dừng" else "▶ Phát"
                }
            }
        }
        val nextBtn = Button(this).apply {
            text = "Tiếp ⏭"
            setOnClickListener { playNext() }
        }
        ctrlRow.addView(prevBtn)
        ctrlRow.addView(Space(this).apply { minimumWidth = 24 })
        ctrlRow.addView(playPauseBtn)
        ctrlRow.addView(Space(this).apply { minimumWidth = 24 })
        ctrlRow.addView(nextBtn)

        bottomBar.addView(nowPlayingTitle)
        bottomBar.addView(nowPlayingTime)
        bottomBar.addView(seekBar)
        bottomBar.addView(ctrlRow)
        root.addView(bottomBar)

        setContentView(root)
    }

    private fun playIndex(idx: Int) {
        if (idx !in allTracks.indices) return
        currentIndex = idx
        val item = allTracks[idx]
        nowPlayingTitle.text = "♫ ${item.title} (${item.formatBadge})"
        playPauseBtn.text = "⏸ Tạm dừng"
        listAdapter.notifyDataSetChanged()
        pcmPlayer.play(item.pathOrUri)
    }

    private fun playNext() {
        if (allTracks.isEmpty()) return
        val next = (currentIndex + 1) % allTracks.size
        playIndex(next)
    }

    private fun playPrev() {
        if (allTracks.isEmpty()) return
        val prev = (currentIndex + allTracks.size - 1) % allTracks.size
        playIndex(prev)
    }

    private fun filterPlaylist() {
        val q = searchInput.text.toString().trim().lowercase()
        filteredTracks.clear()
        for (t in allTracks) {
            if (q.isEmpty() || t.title.lowercase().contains(q) || t.artist.lowercase().contains(q)) {
                filteredTracks.add(t)
            }
        }
        listAdapter.notifyDataSetChanged()
    }

    // ---------------- Parametric EQ / AutoEQ Dialog ----------------
    private fun showParametricEqDialog() {
        val scroll = ScrollView(this)
        val container = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(36, 28, 36, 28)
            setBackgroundColor(Color.parseColor("#14161E"))
        }

        val curveView = EqCurveView(this, eqConfig).apply {
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 280)
        }

        val enableSwitch = CheckBox(this).apply {
            text = "Bật Parametric EQ (64-bit DSP chuẩn Peace EQ / AutoEQ)"
            setTextColor(Color.parseColor("#3DD68C"))
            isChecked = eqConfig.enabled
        }

        val presetSpinner = Spinner(this)
        val presetNames = EqConfig.BUILTIN_PRESETS.map { it.first }
        presetSpinner.adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, presetNames)
        val currentPresetIdx = presetNames.indexOf(eqConfig.name)
        if (currentPresetIdx >= 0) presetSpinner.setSelection(currentPresetIdx)

        val preampLabel = TextView(this).apply {
            text = String.format(java.util.Locale.US, "Preamp: %+.1f dB", eqConfig.preampDb)
            setTextColor(Color.WHITE)
        }
        val preampSeek = SeekBar(this).apply {
            max = 360 // -24.0 to +12.0 dB
            progress = ((eqConfig.preampDb + 24.0) * 10.0).toInt().coerceIn(0, 360)
        }

        val autoPreampBtn = Button(this).apply {
            text = "🛡 Tự động chống rè (Auto Preamp)"
        }

        val autoEqEdit = EditText(this).apply {
            setText(eqConfig.toAutoEqText())
            setTextColor(Color.parseColor("#3DD68C"))
            setBackgroundColor(Color.parseColor("#0D0E12"))
            typeface = Typeface.MONOSPACE
            textSize = 12f
            minLines = 6
        }

        fun syncChangesFromText(name: String, enable: Boolean) {
            eqConfig = EqConfig.fromAutoEqText(name, autoEqEdit.text.toString(), enable)
            enableSwitch.isChecked = eqConfig.enabled
            preampLabel.text = String.format(java.util.Locale.US, "Preamp: %+.1f dB", eqConfig.preampDb)
            preampSeek.progress = ((eqConfig.preampDb + 24.0) * 10.0).toInt().coerceIn(0, 360)
            curveView.updateConfig(eqConfig)
            saveAndApplyEq()
        }

        enableSwitch.setOnCheckedChangeListener { _, checked ->
            eqConfig.enabled = checked
            curveView.updateConfig(eqConfig)
            saveAndApplyEq()
        }

        var spinnerReady = false
        presetSpinner.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
            override fun onItemSelected(parent: AdapterView<*>?, view: View?, position: Int, id: Long) {
                if (!spinnerReady) {
                    spinnerReady = true
                    return
                }
                val chosen = EqConfig.BUILTIN_PRESETS[position]
                autoEqEdit.setText(chosen.second)
                syncChangesFromText(chosen.first, position != 0)
            }
            override fun onNothingSelected(parent: AdapterView<*>?) {}
        }

        preampSeek.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(sb: SeekBar?, progress: Int, fromUser: Boolean) {
                if (fromUser) {
                    eqConfig.preampDb = (progress.toDouble() / 10.0) - 24.0
                    preampLabel.text = String.format(java.util.Locale.US, "Preamp: %+.1f dB", eqConfig.preampDb)
                    autoEqEdit.setText(eqConfig.toAutoEqText())
                    curveView.updateConfig(eqConfig)
                    saveAndApplyEq()
                }
            }
            override fun onStartTrackingTouch(sb: SeekBar?) {}
            override fun onStopTrackingTouch(sb: SeekBar?) {}
        })

        autoPreampBtn.setOnClickListener {
            eqConfig.preampDb = eqConfig.autoPreamp()
            preampLabel.text = String.format(java.util.Locale.US, "Preamp: %+.1f dB", eqConfig.preampDb)
            preampSeek.progress = ((eqConfig.preampDb + 24.0) * 10.0).toInt().coerceIn(0, 360)
            autoEqEdit.setText(eqConfig.toAutoEqText())
            curveView.updateConfig(eqConfig)
            saveAndApplyEq()
        }

        container.addView(enableSwitch)
        container.addView(TextView(this).apply { text = "Chọn Preset AutoEQ:"; setTextColor(Color.LTGRAY) })
        container.addView(presetSpinner)
        container.addView(curveView)
        container.addView(preampLabel)
        container.addView(preampSeek)
        container.addView(autoPreampBtn)
        container.addView(TextView(this).apply {
            text = "Cấu hình chuẩn Peace EQ / AutoEQ (ParametricEQ.txt):"
            setTextColor(Color.LTGRAY)
            setPadding(0, 16, 0, 8)
        })
        container.addView(autoEqEdit)

        scroll.addView(container)

        AlertDialog.Builder(this)
            .setTitle("🎚 Parametric EQ (Sony XZ1 Android 15)")
            .setView(scroll)
            .setPositiveButton("✔ Áp dụng") { _, _ ->
                syncChangesFromText(eqConfig.name, enableSwitch.isChecked)
            }
            .setNegativeButton("Đóng", null)
            .show()
    }

    private fun saveAndApplyEq() {
        pcmPlayer.eqProcessor.updateConfig(eqConfig)
        eqButton.text = if (eqConfig.enabled) "🎚 EQ: ON" else "🎚 AutoEQ"
        getSharedPreferences("flac_xz1_prefs", Context.MODE_PRIVATE).edit()
            .putBoolean("peq_enabled", eqConfig.enabled)
            .putString("peq_name", eqConfig.name)
            .putString("peq_text", eqConfig.toAutoEqText())
            .apply()
    }

    private fun showPcLinkDialog() {
        val input = EditText(this).apply {
            setText(pcServerUrl)
            hint = "http://192.168.1.10:8080"
        }
        AlertDialog.Builder(this)
            .setTitle("💻 Kết nối & Tải Offline từ PC")
            .setMessage("Nhập địa chỉ IP hiện trên nút xanh của app PC (tự động tải nhạc FLAC về máy XZ1 để tắt PC vẫn nghe được):")
            .setView(input)
            .setPositiveButton("Lưu & Đồng bộ") { _, _ ->
                pcServerUrl = input.text.toString().trim().removeSuffix("/")
                if (!pcServerUrl.startsWith("http")) pcServerUrl = "http://$pcServerUrl"
                getSharedPreferences("flac_xz1_prefs", Context.MODE_PRIVATE).edit()
                    .putString("pc_url", pcServerUrl)
                    .apply()
                Thread { syncFromPcServerAsync() }.start()
            }
            .setNegativeButton("Đóng", null)
            .show()
    }

    // ---------------- Android 15 MediaStore Watcher & PC Offline Sync ----------------
    private fun registerMediaStoreWatcher() {
        val observer = object : ContentObserver(uiHandler) {
            override fun onChange(selfChange: Boolean) {
                super.onChange(selfChange)
                Thread { scanMediaStoreAndOfflineIncremental() }.start()
            }
        }
        contentResolver.registerContentObserver(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, true, observer)
        mediaObserver = observer
    }

    private fun startAutoFolderAndServerWatcher() {
        Thread {
            while (!isFinishing) {
                scanMediaStoreAndOfflineIncremental()
                syncFromPcServerAsync()
                try {
                    Thread.sleep(3000)
                } catch (_: Exception) {}
            }
        }.apply { isDaemon = true; start() }
    }

    @Synchronized
    private fun scanMediaStoreAndOfflineIncremental() {
        val newlyFound = mutableListOf<AudioItem>()

        // 1. Scan Android 15 MediaStore (covers Internal Storage + microSD card on Sony XZ1)
        val perm = if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_AUDIO else Manifest.permission.READ_EXTERNAL_STORAGE
        if (ContextCompat.checkSelfPermission(this, perm) == PackageManager.PERMISSION_GRANTED) {
            val projection = arrayOf(
                MediaStore.Audio.Media._ID,
                MediaStore.Audio.Media.TITLE,
                MediaStore.Audio.Media.ARTIST,
                MediaStore.Audio.Media.DURATION,
                MediaStore.Audio.Media.DISPLAY_NAME,
                MediaStore.Audio.Media.MIME_TYPE
            )
            try {
                contentResolver.query(
                    MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,
                    projection,
                    null,
                    null,
                    "${MediaStore.Audio.Media.DATE_ADDED} DESC"
                )?.use { cursor ->
                    val idCol = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media._ID)
                    val titleCol = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.TITLE)
                    val artistCol = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.ARTIST)
                    val durCol = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DURATION)
                    val nameCol = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DISPLAY_NAME)

                    while (cursor.moveToNext()) {
                        val id = cursor.getLong(idCol)
                        val contentUri = ContentUris.withAppendedId(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, id).toString()
                        if (!knownTrackKeys.contains(contentUri)) {
                            knownTrackKeys.add(contentUri)
                            val displayName = cursor.getString(nameCol) ?: ""
                            val ext = displayName.substringAfterLast('.', "FLAC").uppercase()
                            val rawTitle = cursor.getString(titleCol) ?: displayName.substringBeforeLast('.')
                            val rawArtist = cursor.getString(artistCol) ?: "Sony XZ1 Storage"
                            val durMs = cursor.getLong(durCol)
                            newlyFound.add(
                                AudioItem(
                                    title = rawTitle,
                                    artist = if (rawArtist == "<unknown>") "Hi-Res Audio" else rawArtist,
                                    pathOrUri = contentUri,
                                    formatBadge = ext,
                                    durationSec = durMs.toDouble() / 1000.0,
                                    isOfflineSaved = true
                                )
                            )
                        }
                    }
                }
            } catch (_: Exception) {}
        }

        // 2. Scan app-private OfflineAudio folder (downloaded from PC)
        val offlineDir = File(filesDir, "OfflineAudio").apply { mkdirs() }
        offlineDir.listFiles()?.forEach { f ->
            if (f.isFile && f.length() > 0 && !knownTrackKeys.contains(f.absolutePath)) {
                knownTrackKeys.add(f.absolutePath)
                newlyFound.add(
                    AudioItem(
                        title = f.nameWithoutExtension.substringBeforeLast('_'),
                        artist = "Đã lưu từ PC",
                        pathOrUri = f.absolutePath,
                        formatBadge = f.extension.uppercase(),
                        durationSec = 0.0,
                        isOfflineSaved = true
                    )
                )
            }
        }

        if (newlyFound.isNotEmpty()) {
            uiHandler.post {
                allTracks.addAll(newlyFound)
                filterPlaylist()
                statusBanner.text = "✔ Đã nhận ${allTracks.size} bài nhạc Offline trên Sony XZ1 (Tắt PC vẫn nghe được)"
            }
        }
    }

    private fun syncFromPcServerAsync() {
        try {
            val url = URL("$pcServerUrl/api/tracks?t=${System.currentTimeMillis()}")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                connectTimeout = 2000
                readTimeout = 3000
            }
            if (conn.responseCode != 200) return
            val jsonStr = conn.inputStream.bufferedReader().use { it.readText() }
            val arr = JSONArray(jsonStr)
            val offlineDir = File(filesDir, "OfflineAudio").apply { mkdirs() }

            for (i in 0 until arr.length()) {
                val obj = arr.getJSONObject(i)
                val title = obj.optString("title", "Track ${i + 1}")
                val artist = obj.optString("artist", "PC Stream")
                val remotePath = obj.optString("path", "$i")
                val bits = obj.optInt("bits", 16)
                val sr = obj.optInt("sample_rate", 44100) / 1000
                val ext = remotePath.substringAfterLast('.', "flac").lowercase()
                val hash = MessageDigest.getInstance("SHA-256").digest(remotePath.toByteArray())
                    .joinToString("") { "%02x".format(it) }.take(16)
                val safeTitle = title.replace(Regex("[^a-zA-Z0-9_\\- ]"), "").trim().ifEmpty { "track_$i" }
                val targetFile = File(offlineDir, "${safeTitle}_$hash.$ext")

                if (!targetFile.exists()) {
                    val streamUrl = URL("$pcServerUrl/api/stream/$i")
                    streamUrl.openStream().use { input ->
                        targetFile.outputStream().use { output -> input.copyTo(output) }
                    }
                }
                if (targetFile.exists() && targetFile.length() > 0 && !knownTrackKeys.contains(targetFile.absolutePath)) {
                    knownTrackKeys.add(targetFile.absolutePath)
                    val item = AudioItem(
                        title = title,
                        artist = artist,
                        pathOrUri = targetFile.absolutePath,
                        formatBadge = "${ext.uppercase()} ${bits}bit/${sr}kHz",
                        durationSec = obj.optDouble("duration", 0.0),
                        isOfflineSaved = true
                    )
                    uiHandler.post {
                        allTracks.add(item)
                        filterPlaylist()
                        statusBanner.text = "✔ Đã tự lưu Offline ${allTracks.size} bài vào XZ1 (Tắt PC vẫn nghe được)"
                    }
                }
            }
        } catch (_: Exception) {}
    }

    private fun requestAudioPermissionAndScan() {
        val perms = if (Build.VERSION.SDK_INT >= 33) {
            arrayOf(Manifest.permission.READ_MEDIA_AUDIO)
        } else {
            arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
        }
        if (ContextCompat.checkSelfPermission(this, perms[0]) != PackageManager.PERMISSION_GRANTED) {
            ActivityCompat.requestPermissions(this, perms, 101)
        } else {
            Thread { scanMediaStoreAndOfflineIncremental() }.start()
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 101 && grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED) {
            Thread { scanMediaStoreAndOfflineIncremental() }.start()
        }
    }

    private fun fmtTime(sec: Double): String {
        val s = sec.toInt().coerceAtLeast(0)
        return String.format(java.util.Locale.US, "%d:%02d", s / 60, s % 60)
    }

    override fun onDestroy() {
        super.onDestroy()
        mediaObserver?.let { contentResolver.unregisterContentObserver(it) }
        pcmPlayer.stop()
    }
}

class EqCurveView(context: Context, private var config: EqConfig) : View(context) {
    private val bgPaint = Paint().apply { color = Color.parseColor("#0B0D12"); style = Paint.Style.FILL }
    private val gridPaint = Paint().apply { color = Color.parseColor("#2A303D"); strokeWidth = 2f }
    private val curvePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#4CC2FF")
        strokeWidth = 5f
        style = Paint.Style.STROKE
    }

    fun updateConfig(newCfg: EqConfig) {
        config = newCfg
        curvePaint.color = if (config.enabled) Color.parseColor("#4CC2FF") else Color.GRAY
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val w = width.toFloat()
        val h = height.toFloat()
        canvas.drawRect(0f, 0f, w, h, bgPaint)
        canvas.drawLine(0f, h / 2f, w, h / 2f, gridPaint)

        val path = Path()
        val steps = 100
        val dbRange = 18.0
        for (i in 0 until steps) {
            val t = i.toDouble() / (steps - 1).toDouble()
            val freq = 20.0 * 1000.0.pow(t)
            val db = config.responseDb(freq, 48000.0).coerceIn(-dbRange, dbRange)
            val x = (t * w).toFloat()
            val y = (h / 2f) - ((db / dbRange) * (h * 0.42f)).toFloat()
            if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
        }
        canvas.drawPath(path, curvePaint)
    }
}
