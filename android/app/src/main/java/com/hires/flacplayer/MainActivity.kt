package com.hires.flacplayer

import android.Manifest
import android.app.AlertDialog
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.database.ContentObserver
import android.graphics.*
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.MediaStore
import android.text.Editable
import android.text.TextUtils
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
import kotlin.math.ln
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
    private lateinit var eqButton: TextView
    private lateinit var searchInput: EditText
    private lateinit var listView: ListView
    private lateinit var listAdapter: BaseAdapter
    private lateinit var nowPlayingTitle: TextView
    private lateinit var nowPlayingSub: TextView
    private lateinit var currTimeTv: TextView
    private lateinit var totalTimeTv: TextView
    private lateinit var seekBar: SeekBar
    private lateinit var playPauseBtn: TextView
    private var isUserSeeking = false

    private var pcServerUrl: String = "http://192.168.1.10:8080"
    private val knownTrackKeys = HashSet<String>()
    private var mediaObserver: ContentObserver? = null

    private fun dp(v: Int): Int = (v * resources.displayMetrics.density).toInt()

    private fun roundedBg(fillHex: String, cornerDp: Int, strokeHex: String? = null, strokeWidthDp: Int = 1): GradientDrawable {
        return GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = dp(cornerDp).toFloat()
            setColor(Color.parseColor(fillHex))
            if (strokeHex != null) {
                setStroke(dp(strokeWidthDp), Color.parseColor(strokeHex))
            }
        }
    }

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
                        currTimeTv.text = fmtTime(pos)
                        totalTimeTv.text = fmtTime(dur)
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
            setBackgroundColor(Color.parseColor("#0B0D12"))
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        }

        // 1. Sleek Top Header
        val topBar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setBackgroundColor(Color.parseColor("#12151E"))
            setPadding(dp(16), dp(14), dp(16), dp(12))
        }
        val titleWrap = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
        }
        val titleTv = TextView(this).apply {
            text = "◉ Hi-Res Audio"
            textSize = 18f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#4CC2FF"))
        }
        val subTitleTv = TextView(this).apply {
            text = "Sony Xperia XZ1 • 64-bit DSP"
            textSize = 11f
            setTextColor(Color.parseColor("#7E8696"))
        }
        titleWrap.addView(titleTv)
        titleWrap.addView(subTitleTv)

        eqButton = TextView(this).apply {
            gravity = Gravity.CENTER
            setPadding(dp(14), dp(8), dp(14), dp(8))
            textSize = 13f
            setTypeface(typeface, Typeface.BOLD)
            setOnClickListener { showParametricEqDialog() }
        }
        updateEqButtonStyle()

        val pcBtn = TextView(this).apply {
            text = "📡 PC Sync"
            gravity = Gravity.CENTER
            setPadding(dp(14), dp(8), dp(14), dp(8))
            textSize = 13f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#3DD68C"))
            background = roundedBg("#162621", 18, "#28483D")
            setOnClickListener { showPcLinkDialog() }
        }

        topBar.addView(titleWrap)
        topBar.addView(eqButton)
        topBar.addView(Space(this).apply { minimumWidth = dp(8) })
        topBar.addView(pcBtn)
        root.addView(topBar)

        // 2. Search & Status Area
        val headerSub = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.parseColor("#12151E"))
            setPadding(dp(16), 0, dp(16), dp(12))
        }
        statusBanner = TextView(this).apply {
            text = "🔄 Đang quét kho nhạc trên máy & thẻ nhớ microSD..."
            textSize = 11.5f
            setTextColor(Color.parseColor("#3DD68C"))
            background = roundedBg("#15221E", 8, "#233B33")
            setPadding(dp(12), dp(7), dp(12), dp(7))
        }
        searchInput = EditText(this).apply {
            hint = "🔍 Tìm bài hát, nghệ sĩ, album..."
            setHintTextColor(Color.parseColor("#6B7280"))
            setTextColor(Color.WHITE)
            textSize = 14f
            setSingleLine(true)
            background = roundedBg("#1A1E29", 10, "#2A3142")
            setPadding(dp(14), dp(10), dp(14), dp(10))
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(8)
            }
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) { filterPlaylist() }
                override fun afterTextChanged(s: Editable?) {}
            })
        }
        headerSub.addView(statusBanner)
        headerSub.addView(searchInput)
        root.addView(headerSub)

        // 3. Track ListView
        listView = ListView(this).apply {
            divider = null
            dividerHeight = dp(4)
            setPadding(dp(12), dp(8), dp(12), dp(8))
            clipToPadding = false
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)
        }
        listAdapter = object : BaseAdapter() {
            override fun getCount(): Int = filteredTracks.size
            override fun getItem(position: Int): Any = filteredTracks[position]
            override fun getItemId(position: Int): Long = position.toLong()
            override fun getView(position: Int, convertView: View?, parent: ViewGroup?): View {
                val item = filteredTracks[position]
                val active = (currentIndex in allTracks.indices && allTracks[currentIndex].pathOrUri == item.pathOrUri)

                val row = LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER_VERTICAL
                    setPadding(dp(14), dp(12), dp(14), dp(12))
                    background = if (active) {
                        roundedBg("#172638", 12, "#4CC2FF")
                    } else {
                        roundedBg("#131720", 12, "#1E2330")
                    }
                }

                val numTv = TextView(this@MainActivity).apply {
                    text = if (active) "▶" else "${position + 1}"
                    textSize = 13f
                    gravity = Gravity.CENTER
                    setTypeface(Typeface.MONOSPACE, Typeface.BOLD)
                    setTextColor(if (active) Color.parseColor("#4CC2FF") else Color.parseColor("#6B7280"))
                    layoutParams = LinearLayout.LayoutParams(dp(28), ViewGroup.LayoutParams.WRAP_CONTENT)
                }

                val infoCol = LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply {
                        marginStart = dp(8)
                        marginEnd = dp(8)
                    }
                }
                val titleView = TextView(this@MainActivity).apply {
                    text = item.title
                    textSize = 15f
                    maxLines = 1
                    ellipsize = TextUtils.TruncateAt.END
                    setTypeface(typeface, if (active) Typeface.BOLD else Typeface.NORMAL)
                    setTextColor(if (active) Color.parseColor("#4CC2FF") else Color.WHITE)
                }
                val subView = TextView(this@MainActivity).apply {
                    text = item.artist
                    textSize = 12f
                    maxLines = 1
                    ellipsize = TextUtils.TruncateAt.END
                    setTextColor(Color.parseColor("#9AA0A6"))
                }
                infoCol.addView(titleView)
                infoCol.addView(subView)

                val rightCol = LinearLayout(this@MainActivity).apply {
                    orientation = LinearLayout.VERTICAL
                    gravity = Gravity.END
                }
                val badgeTv = TextView(this@MainActivity).apply {
                    text = item.formatBadge
                    textSize = 10f
                    setTypeface(typeface, Typeface.BOLD)
                    val isHiRes = item.formatBadge.contains("FLAC") || item.formatBadge.contains("24")
                    setTextColor(if (isHiRes) Color.parseColor("#F5C542") else Color.parseColor("#9AA0A6"))
                    background = roundedBg(if (isHiRes) "#262114" else "#1D212C", 6, if (isHiRes) "#59481E" else "#2E3446")
                    setPadding(dp(6), dp(2), dp(6), dp(2))
                }
                val durTv = TextView(this@MainActivity).apply {
                    text = if (item.durationSec > 0) fmtTime(item.durationSec) else "Offline"
                    textSize = 11f
                    typeface = Typeface.MONOSPACE
                    setTextColor(Color.parseColor("#7E8696"))
                    setPadding(0, dp(4), 0, 0)
                }
                rightCol.addView(badgeTv)
                rightCol.addView(durTv)

                row.addView(numTv)
                row.addView(infoCol)
                row.addView(rightCol)
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

        // 4. Elevated Now Playing Bottom Dock
        val bottomBar = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = roundedBg("#151924", 18, "#262D3F")
            setPadding(dp(18), dp(14), dp(18), dp(18))
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                setMargins(dp(10), dp(2), dp(10), dp(10))
            }
        }

        nowPlayingTitle = TextView(this).apply {
            text = "Chưa phát bài nào"
            textSize = 15f
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.WHITE)
        }
        nowPlayingSub = TextView(this).apply {
            text = "Chọn một bài hát để thưởng thức âm thanh Hi-Res"
            textSize = 12f
            maxLines = 1
            ellipsize = TextUtils.TruncateAt.END
            setTextColor(Color.parseColor("#4CC2FF"))
            setPadding(0, dp(2), 0, dp(8))
        }

        // Scrubber Row with Left/Right Time
        val seekRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        currTimeTv = TextView(this).apply {
            text = "0:00"
            textSize = 11f
            typeface = Typeface.MONOSPACE
            setTextColor(Color.parseColor("#8A93A6"))
        }
        seekBar = SeekBar(this).apply {
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(sb: SeekBar?, progress: Int, fromUser: Boolean) {
                    if (fromUser) currTimeTv.text = fmtTime(progress.toDouble() / 1000.0)
                }
                override fun onStartTrackingTouch(sb: SeekBar?) { isUserSeeking = true }
                override fun onStopTrackingTouch(sb: SeekBar?) {
                    isUserSeeking = false
                    sb?.let { pcmPlayer.seekToSeconds(it.progress.toDouble() / 1000.0) }
                }
            })
        }
        totalTimeTv = TextView(this).apply {
            text = "0:00"
            textSize = 11f
            typeface = Typeface.MONOSPACE
            setTextColor(Color.parseColor("#8A93A6"))
        }
        seekRow.addView(currTimeTv)
        seekRow.addView(seekBar)
        seekRow.addView(totalTimeTv)

        // Transport Controls Row
        val ctrlRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            setPadding(0, dp(10), 0, 0)
        }
        val prevBtn = TextView(this).apply {
            text = "⏮  Trước"
            gravity = Gravity.CENTER
            textSize = 13f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.WHITE)
            background = roundedBg("#222838", 20, "#31394F")
            setPadding(dp(18), dp(10), dp(18), dp(10))
            setOnClickListener { playPrev() }
        }
        playPauseBtn = TextView(this).apply {
            text = "▶  Phát nhạc"
            gravity = Gravity.CENTER
            textSize = 14f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#0B0D12"))
            background = roundedBg("#4CC2FF", 22)
            setPadding(dp(28), dp(11), dp(28), dp(11))
            setOnClickListener {
                if (currentIndex == -1 && allTracks.isNotEmpty()) {
                    playIndex(0)
                } else {
                    val playing = pcmPlayer.togglePause()
                    text = if (playing) "⏸  Tạm dừng" else "▶  Tiếp tục"
                }
            }
        }
        val nextBtn = TextView(this).apply {
            text = "Tiếp  ⏭"
            gravity = Gravity.CENTER
            textSize = 13f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.WHITE)
            background = roundedBg("#222838", 20, "#31394F")
            setPadding(dp(18), dp(10), dp(18), dp(10))
            setOnClickListener { playNext() }
        }
        ctrlRow.addView(prevBtn)
        ctrlRow.addView(Space(this).apply { minimumWidth = dp(16) })
        ctrlRow.addView(playPauseBtn)
        ctrlRow.addView(Space(this).apply { minimumWidth = dp(16) })
        ctrlRow.addView(nextBtn)

        bottomBar.addView(nowPlayingTitle)
        bottomBar.addView(nowPlayingSub)
        bottomBar.addView(seekRow)
        bottomBar.addView(ctrlRow)
        root.addView(bottomBar)

        setContentView(root)
    }

    private fun updateEqButtonStyle() {
        if (eqConfig.enabled) {
            eqButton.text = "🎚 EQ: ON"
            eqButton.setTextColor(Color.parseColor("#0B0D12"))
            eqButton.background = roundedBg("#F5C542", 18)
        } else {
            eqButton.text = "🎚 Parametric EQ"
            eqButton.setTextColor(Color.parseColor("#E5E7EB"))
            eqButton.background = roundedBg("#202532", 18, "#32394D")
        }
    }

    private fun playIndex(idx: Int) {
        if (idx !in allTracks.indices) return
        currentIndex = idx
        val item = allTracks[idx]
        nowPlayingTitle.text = item.title
        nowPlayingSub.text = "${item.artist} • ${item.formatBadge}"
        playPauseBtn.text = "⏸  Tạm dừng"
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
            setPadding(dp(20), dp(16), dp(20), dp(16))
            setBackgroundColor(Color.parseColor("#13161F"))
        }

        val curveView = EqCurveView(this, eqConfig).apply {
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(155)).apply {
                bottomMargin = dp(12)
            }
        }

        val enableSwitch = CheckBox(this).apply {
            text = "Bật Parametric EQ (64-bit Biquad DSP)"
            textSize = 14f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#3DD68C"))
            isChecked = eqConfig.enabled
        }

        val presetLabel = TextView(this).apply {
            text = "Chọn cấu hình AutoEQ:"
            textSize = 12f
            setTextColor(Color.parseColor("#9AA0A6"))
            setPadding(0, dp(8), 0, dp(4))
        }
        val presetSpinner = Spinner(this).apply {
            background = roundedBg("#1D2230", 8, "#2F374C")
            setPadding(dp(8), dp(4), dp(8), dp(4))
        }
        val presetNames = EqConfig.BUILTIN_PRESETS.map { it.first }
        presetSpinner.adapter = ArrayAdapter(this, android.R.layout.simple_spinner_dropdown_item, presetNames)
        val currentPresetIdx = presetNames.indexOf(eqConfig.name)
        if (currentPresetIdx >= 0) presetSpinner.setSelection(currentPresetIdx)

        val preampLabel = TextView(this).apply {
            text = String.format(java.util.Locale.US, "Preamp (Chống rè): %+.1f dB", eqConfig.preampDb)
            textSize = 13f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.WHITE)
            setPadding(0, dp(10), 0, dp(4))
        }
        val preampSeek = SeekBar(this).apply {
            max = 360 // -24.0 to +12.0 dB
            progress = ((eqConfig.preampDb + 24.0) * 10.0).toInt().coerceIn(0, 360)
        }

        val autoPreampBtn = TextView(this).apply {
            text = "🛡 Tự động cân chỉnh chống rè (Auto-Preamp)"
            gravity = Gravity.CENTER
            textSize = 12.5f
            setTypeface(typeface, Typeface.BOLD)
            setTextColor(Color.parseColor("#F5C542"))
            background = roundedBg("#262115", 10, "#524321")
            setPadding(dp(12), dp(9), dp(12), dp(9))
            layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                topMargin = dp(8)
            }
        }

        val autoEqEdit = EditText(this).apply {
            setText(eqConfig.toAutoEqText())
            setTextColor(Color.parseColor("#3DD68C"))
            background = roundedBg("#0B0D12", 10, "#262C3D")
            setPadding(dp(12), dp(10), dp(12), dp(10))
            typeface = Typeface.MONOSPACE
            textSize = 12f
            minLines = 6
        }

        fun syncChangesFromText(name: String, enable: Boolean) {
            eqConfig = EqConfig.fromAutoEqText(name, autoEqEdit.text.toString(), enable)
            enableSwitch.isChecked = eqConfig.enabled
            preampLabel.text = String.format(java.util.Locale.US, "Preamp (Chống rè): %+.1f dB", eqConfig.preampDb)
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
                    preampLabel.text = String.format(java.util.Locale.US, "Preamp (Chống rè): %+.1f dB", eqConfig.preampDb)
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
            preampLabel.text = String.format(java.util.Locale.US, "Preamp (Chống rè): %+.1f dB", eqConfig.preampDb)
            preampSeek.progress = ((eqConfig.preampDb + 24.0) * 10.0).toInt().coerceIn(0, 360)
            autoEqEdit.setText(eqConfig.toAutoEqText())
            curveView.updateConfig(eqConfig)
            saveAndApplyEq()
        }

        container.addView(curveView)
        container.addView(enableSwitch)
        container.addView(presetLabel)
        container.addView(presetSpinner)
        container.addView(preampLabel)
        container.addView(preampSeek)
        container.addView(autoPreampBtn)
        container.addView(TextView(this).apply {
            text = "Thông số AutoEQ (ParametricEQ.txt):"
            textSize = 12f
            setTextColor(Color.parseColor("#9AA0A6"))
            setPadding(0, dp(14), 0, dp(6))
        })
        container.addView(autoEqEdit)

        scroll.addView(container)

        AlertDialog.Builder(this)
            .setTitle("🎚 Parametric EQ • 64-bit DSP")
            .setView(scroll)
            .setPositiveButton("✔ Áp dụng") { _, _ ->
                syncChangesFromText(eqConfig.name, enableSwitch.isChecked)
            }
            .setNegativeButton("Đóng", null)
            .show()
    }

    private fun saveAndApplyEq() {
        pcmPlayer.eqProcessor.updateConfig(eqConfig)
        updateEqButtonStyle()
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
            setPadding(dp(16), dp(12), dp(16), dp(12))
        }
        AlertDialog.Builder(this)
            .setTitle("📡 Đồng bộ nhạc Offline từ PC")
            .setMessage("Nhập địa chỉ IP hiển thị trên thanh công cụ của ứng dụng PC để tự động lưu nhạc FLAC về máy:")
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
                MediaStore.Audio.Media.DISPLAY_NAME
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
                            val rawArtist = cursor.getString(artistCol) ?: "Hi-Res Audio"
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
                        artist = "Đã đồng bộ từ PC",
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
                statusBanner.text = "✔ Sẵn sàng ${allTracks.size} bài nhạc Offline trên máy (Nghe mọi lúc không cần PC)"
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
                val artist = obj.optString("artist", "PC Library")
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
                        statusBanner.text = "✔ Đã lưu Offline ${allTracks.size} bài vào bộ nhớ máy"
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
    private val bgPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.parseColor("#0B0D12"); style = Paint.Style.FILL }
    private val borderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.parseColor("#262D3F"); strokeWidth = 3f; style = Paint.Style.STROKE }
    private val subGridPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.parseColor("#1A1F2C"); strokeWidth = 2f }
    private val zeroGridPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.parseColor("#384259"); strokeWidth = 2.5f }
    private val labelPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#7E8696")
        textSize = 22f
        typeface = Typeface.MONOSPACE
    }
    private val curvePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#4CC2FF")
        strokeWidth = 5.5f
        style = Paint.Style.STROKE
    }
    private val dotPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#F5C542")
        style = Paint.Style.FILL
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
        val rect = RectF(2f, 2f, w - 2f, h - 2f)
        canvas.drawRoundRect(rect, 18f, 18f, bgPaint)
        canvas.drawRoundRect(rect, 18f, 18f, borderPaint)

        val plotTop = 16f
        val plotBottom = h - 28f
        val midY = (plotTop + plotBottom) * 0.5f
        val halfH = (plotBottom - plotTop) * 0.44f
        val dbRange = 18.0

        // Frequency vertical grid lines
        for ((fMark, lbl) in listOf(63.0 to "63", 250.0 to "250", 1000.0 to "1k", 4000.0 to "4k", 16000.0 to "16k")) {
            val t = (ln(fMark / 20.0) / ln(1000.0)).toFloat()
            val x = t * w
            canvas.drawLine(x, plotTop, x, plotBottom, subGridPaint)
            canvas.drawText(lbl, x - 16f, h - 6f, labelPaint)
        }

        // Horizontal dB lines
        for (dbLine in listOf(-12.0, -6.0, 0.0, 6.0, 12.0)) {
            val y = midY - ((dbLine / dbRange) * halfH).toFloat()
            canvas.drawLine(0f, y, w, y, if (dbLine == 0.0) zeroGridPaint else subGridPaint)
        }

        // Response curve
        val path = Path()
        val steps = 120
        for (i in 0 until steps) {
            val t = i.toDouble() / (steps - 1).toDouble()
            val freq = 20.0 * 1000.0.pow(t)
            val db = config.responseDb(freq, 48000.0).coerceIn(-dbRange, dbRange)
            val x = (t * w).toFloat()
            val y = midY - ((db / dbRange) * halfH).toFloat()
            if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
        }
        canvas.drawPath(path, curvePaint)

        // Band dots
        for (b in config.bands) {
            if (!b.enabled) continue
            val f = b.freq.coerceIn(20.0, 20000.0)
            val t = (ln(f / 20.0) / ln(1000.0)).toFloat()
            val db = config.responseDb(f, 48000.0).coerceIn(-dbRange, dbRange)
            val x = t * w
            val y = midY - ((db / dbRange) * halfH).toFloat()
            canvas.drawCircle(x, y, 8f, dotPaint)
        }
    }
}
