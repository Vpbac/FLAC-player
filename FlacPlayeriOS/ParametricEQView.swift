import SwiftUI

public struct ParametricEQView: View {
    @ObservedObject var eq = EqManager.shared
    @Environment(\.presentationMode) var presentationMode
    @State private var importText: String = ""
    @State private var showImportSheet: Bool = false

    public var body: some View {
        NavigationView {
            Form {
                Section(header: Text("Trạng Thái & Preset AutoEQ")) {
                    Toggle(isOn: $eq.isEnabled) {
                        Text("Bật Parametric EQ (64-bit DSP)")
                            .fontWeight(.semibold)
                    }

                    Picker("Preset AutoEQ", selection: $eq.presetName) {
                        ForEach(eq.builtinPresets, id: \.name) { item in
                            Text(item.name).tag(item.name)
                        }
                    }
                    .onChange(of: eq.presetName) { newName in
                        if let found = eq.builtinPresets.first(where: { $0.name == newName }) {
                            eq.loadFromAutoEQText(name: found.name, text: found.text, enable: true)
                        }
                    }

                    Button(action: {
                        importText = eq.toAutoEQText()
                        showImportSheet = true
                    }) {
                        HStack {
                            Image(systemName: "doc.text.fill")
                            Text("Dán / Xuất cấu hình Peace EQ (ParametricEQ.txt)")
                        }
                    }
                }

                Section(header: Text("Đồ Thị Đáp Tuyến Tần Số (20Hz - 20kHz)")) {
                    EqCurveGraphView(eq: eq)
                        .frame(height: 130)
                        .padding(.vertical, 4)

                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text("Preamp (Chống rè):")
                            Spacer()
                            Text(String(format: "%+.1f dB", eq.preampDb))
                                .font(.system(.body, design: .monospaced))
                                .foregroundColor(.accentColor)
                        }
                        Slider(value: $eq.preampDb, in: -18.0...12.0, step: 0.5)
                    }
                }

                Section(header: HStack {
                    Text("Các Dải Tần (Parametric Bands)")
                    Spacer()
                    Button(action: {
                        eq.bands.append(EqBand(enabled: true, filterType: .peaking, freq: 1000, gainDb: 0, q: 1.41))
                    }) {
                        Label("Thêm Band", systemImage: "plus.circle.fill")
                            .font(.caption)
                    }
                }) {
                    ForEach(eq.bands.indices, id: \.self) { i in
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                Toggle("#\(i + 1) \(eq.bands[i].filterType.rawValue) • \(Int(eq.bands[i].freq)) Hz", isOn: $eq.bands[i].enabled)
                                    .font(.system(size: 14, weight: .bold))
                                Spacer()
                                Text(String(format: "%+.1f dB (Q %.2f)", eq.bands[i].gainDb, eq.bands[i].q))
                                    .font(.system(size: 12, design: .monospaced))
                                    .foregroundColor(.secondary)
                            }
                            HStack {
                                Text("Fc")
                                    .font(.caption)
                                    .frame(width: 24)
                                Slider(value: $eq.bands[i].freq, in: 20...20000, step: 5)
                            }
                            HStack {
                                Text("dB")
                                    .font(.caption)
                                    .frame(width: 24)
                                Slider(value: $eq.bands[i].gainDb, in: -15...15, step: 0.2)
                            }
                            HStack {
                                Text("Q")
                                    .font(.caption)
                                    .frame(width: 24)
                                Slider(value: $eq.bands[i].q, in: 0.2...10.0, step: 0.1)
                            }
                        }
                        .padding(.vertical, 4)
                    }
                    .onDelete { offsets in
                        eq.bands.remove(atOffsets: offsets)
                    }
                }
            }
            .navigationTitle("Parametric EQ")
            .navigationBarItems(trailing: Button("Xong") {
                presentationMode.wrappedValue.dismiss()
            })
            .sheet(isPresented: $showImportSheet) {
                NavigationView {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Dán nội dung file ParametricEQ.txt từ AutoEQ.app hoặc Peace EQ:")
                            .font(.subheadline)
                            .foregroundColor(.secondary)

                        TextEditor(text: $importText)
                            .font(.system(size: 13, design: .monospaced))
                            .padding(8)
                            .background(Color.black.opacity(0.2))
                            .cornerRadius(8)

                        Button(action: {
                            eq.loadFromAutoEQText(name: "Custom AutoEQ", text: importText, enable: true)
                            showImportSheet = false
                        }) {
                            Text("✔ Áp dụng cấu hình AutoEQ")
                                .fontWeight(.bold)
                                .frame(maxWidth: .infinity)
                                .padding()
                                .background(Color.green)
                                .foregroundColor(.black)
                                .cornerRadius(10)
                        }
                    }
                    .padding()
                    .navigationTitle("Nhập ParametricEQ.txt")
                    .navigationBarItems(trailing: Button("Đóng") {
                        showImportSheet = false
                    })
                }
            }
        }
    }
}

struct EqCurveGraphView: View {
    @ObservedObject var eq: EqManager

    var body: some View {
        GeometryReader { geo in
            ZStack {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color(red: 0.07, green: 0.08, blue: 0.11))

                // 0 dB center line
                Path { path in
                    let midY = geo.size.height / 2
                    path.move(to: CGPoint(x: 0, y: midY))
                    path.addLine(to: CGPoint(x: geo.size.width, y: midY))
                }
                .stroke(Color.gray.opacity(0.4), lineWidth: 1)

                // Response curve
                Path { path in
                    let steps = 100
                    let dbRange = 18.0
                    for i in 0..<steps {
                        let t = Double(i) / Double(steps - 1)
                        let freq = 20.0 * pow(1000.0, t)
                        let db = min(max(eq.responseDb(at: freq), -dbRange), dbRange)
                        let x = CGFloat(t) * geo.size.width
                        let y = (geo.size.height / 2) - CGFloat(db / dbRange) * (geo.size.height * 0.44)
                        if i == 0 {
                            path.move(to: CGPoint(x: x, y: y))
                        } else {
                            path.addLine(to: CGPoint(x: x, y: y))
                        }
                    }
                }
                .stroke(eq.isEnabled ? Color.accentColor : Color.gray, lineWidth: 2.2)
            }
        }
    }
}
