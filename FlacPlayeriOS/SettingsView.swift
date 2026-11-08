import SwiftUI

public struct SettingsView: View {
    @ObservedObject var server = ServerManager.shared
    @State private var inputURL: String = ""
    @Environment(\.presentationMode) var presentationMode

    public var body: some View {
        NavigationView {
            Form {
                Section(header: Text("Địa chỉ PC Server")) {
                    HStack {
                        TextField("http://192.168.1.10:8080", text: $inputURL)
                            .keyboardType(.URL)
                            .autocapitalization(.none)
                            .disableAutocorrection(true)

                        if server.isConnecting {
                            ProgressView()
                        } else {
                            Circle()
                                .fill(server.isConnected ? Color.green : Color.red)
                                .frame(width: 12, height: 12)
                        }
                    }

                    Button("Kiểm tra & Kết nối") {
                        server.serverURLString = inputURL
                        server.testConnection()
                    }
                    .foregroundColor(.accentColor)
                }

                if let err = server.errorMessage {
                    Section {
                        Text(err)
                            .foregroundColor(.red)
                            .font(.system(size: 13))
                    }
                }

                Section(header: Text("Hướng dẫn kết nối từ iPhone")) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("1. Kết nối iPhone vào cùng mạng Wi-Fi với PC.")
                            .font(.system(size: 14))

                        Text("2. Mở app Hi-Res Player trên PC, bấm nút màu xanh '📱 iPhone Link' ở thanh trên cùng để xem địa chỉ IP.")
                            .font(.system(size: 14))

                        Text("3. Nhập chính xác địa chỉ đó vào ô trên (ví dụ: http://192.168.1.10:8080).")
                            .font(.system(size: 14))

                        Text("💡 Lưu ý: Không dùng 'localhost' vì trên iPhone nó trỏ về chính điện thoại của bạn, chứ không trỏ về PC.")
                            .font(.system(size: 13))
                            .foregroundColor(.secondary)
                            .padding(.top, 4)
                    }
                    .padding(.vertical, 4)
                }

                Section(header: Text("Thông tin ứng dụng")) {
                    HStack {
                        Text("Phiên bản")
                        Spacer()
                        Text("1.0.0 (Native SwiftUI)")
                            .foregroundColor(.secondary)
                    }
                    HStack {
                        Text("Hỗ trợ định dạng")
                        Spacer()
                        Text("FLAC, WAV, ALAC, Hi-Res")
                            .foregroundColor(.secondary)
                    }
                }
            }
            .navigationTitle("Cài đặt Máy chủ")
            .navigationBarItems(trailing: Button("Xong") {
                presentationMode.wrappedValue.dismiss()
            })
            .onAppear {
                inputURL = server.serverURLString
            }
        }
    }
}
