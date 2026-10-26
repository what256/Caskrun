import SwiftUI
import Metal
import MetalFX

struct DXMTOptionsView: View {
    let status: BottleGraphicsStatus
    let isBusy: Bool
    let update: (UInt16, UInt16) -> Void
    private static let spatialSupported: Bool = {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        return MTLFXSpatialScalerDescriptor.supportsDevice(device)
    }()

    var body: some View {
        DisclosureGroup("DXMT tuning · Experimental") {
            VStack(alignment: .leading, spacing: 10) {
                Picker("Frame target", selection: Binding(
                    get: { status.dxmt_frame_rate },
                    set: { update($0, status.dxmt_spatial_scale) })) {
                    Text("Application controlled").tag(UInt16(0))
                    ForEach([30, 60, 120], id: \.self) { Text("\($0) FPS").tag(UInt16($0)) }
                    if ![0, 30, 60, 120].contains(status.dxmt_frame_rate) {
                        Text("\(status.dxmt_frame_rate) FPS").tag(status.dxmt_frame_rate)
                    }
                }.pickerStyle(.menu)
                Text("A pacing request, not a guaranteed frame rate. Choose a divisor of your display's refresh rate. Games can impose their own limits.")
                    .font(.caption).foregroundStyle(.secondary)
                if status.dxmt_spatial_available {
                    Picker("MetalFX spatial", selection: Binding(
                        get: { status.dxmt_spatial_scale },
                        set: { if $0 == 100 || Self.spatialSupported { update(status.dxmt_frame_rate, $0) } })) {
                        Text("Off").tag(UInt16(100))
                        if Self.spatialSupported {
                            Text("1.25×").tag(UInt16(125))
                            Text("1.5×").tag(UInt16(150))
                            Text("2×").tag(UInt16(200))
                        } else if status.dxmt_spatial_scale != 100 {
                            Text("Saved scale · Unavailable here").tag(status.dxmt_spatial_scale).disabled(true)
                        }
                    }.pickerStyle(.menu)
                    Text(Self.spatialSupported
                         ? (status.dxmt_d3d12_available == true
                            ? "Upscales DirectX 10/11 presentation only; experimental DirectX 12 does not use this setting. Test image quality and performance; this does not enable DLSS or frame generation."
                            : "Upscales the presentation surface. Test image quality and performance in your game; this does not enable DLSS or frame generation.")
                         : "MetalFX spatial scaling is unavailable on this Mac. Saved settings are preserved; choose Off or reset tuning before playing here.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Text("Nondefault tuning takes precedence over a custom dxmt.conf for that launch. Reset both controls to restore the original config behavior. Close this bottle's apps before changing options.")
                    .font(.caption).foregroundStyle(.secondary)
                Button("Reset DXMT tuning") { update(0, 100) }
                    .buttonStyle(.borderless).font(.caption)
                    .disabled(status.dxmt_frame_rate == 0 && status.dxmt_spatial_scale == 100)
            }.padding(.top, 8)
        }
        .font(.caption).disabled(isBusy || !status.dxmt_available)
    }
}
