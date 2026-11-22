import AppKit

// Renders an Apprentice SVG at an exact pixel size. `ink` substitutes the
// __INK__ placeholder; `raw` writes premultiplication-free RGBA bytes for the
// Windows tray (the tray tints the shape itself and needs clean alpha).

let args = CommandLine.arguments
guard args.count >= 5,
      let width = Int(args[2]), let height = Int(args[3]),
      width > 0, height > 0 else {
    FileHandle.standardError.write(Data("usage: render-apprentice-icon.swift <svg> <w> <h> <out> [ink] [raw|png]\n".utf8))
    exit(2)
}
let source = args[1]
let output = args[4]
let ink = args.count > 5 ? args[5] : "#ffffff"
let mode = args.count > 6 ? args[6] : "png"
// canvas: the image is drawn at width x height, centred in a larger transparent
// output (Icon Composer's layer asset uses a 1488 x 1672 template canvas).
let canvas = mode == "canvas" && args.count > 8
    ? (w: Int(args[7]) ?? width, h: Int(args[8]) ?? height)
    : nil
let raw = mode == "raw"

guard var svg = try? String(contentsOfFile: source, encoding: .utf8),
      let data = svg.replacingOccurrences(of: "__INK__", with: ink).data(using: .utf8),
      let image = NSImage(data: data) else {
    FileHandle.standardError.write(Data("could not load \(source)\n".utf8))
    exit(1)
}

let outWidth = canvas?.w ?? width
let outHeight = canvas?.h ?? height
let originX = canvas.map { Double($0.w - width) / 2 } ?? 0
let originY = canvas.map { Double($0.h - height) / 2 } ?? 0
let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: outWidth, pixelsHigh: outHeight,
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
image.draw(in: NSRect(x: originX, y: originY, width: Double(width), height: Double(height)))
NSGraphicsContext.restoreGraphicsState()

func channels(_ hex: String) -> (UInt8, UInt8, UInt8) {
    let text = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
    guard text.count == 6, let value = UInt32(text, radix: 16) else { return (255, 255, 255) }
    return (UInt8((value >> 16) & 0xff), UInt8((value >> 8) & 0xff), UInt8(value & 0xff))
}

var bytes = Data()
if raw {
    let (r, g, b) = channels(ink)
    let pixels = rep.bitmapData!
    bytes.reserveCapacity(outWidth * outHeight * 4)
    for index in stride(from: 0, to: outWidth * outHeight * 4, by: 4) {
        let alpha = pixels[index + 3]
        bytes.append(alpha == 0 ? 0 : r)
        bytes.append(alpha == 0 ? 0 : g)
        bytes.append(alpha == 0 ? 0 : b)
        bytes.append(alpha)
    }
} else {
    guard let png = rep.representation(using: .png, properties: [:]) else {
        FileHandle.standardError.write(Data("could not encode \(output)\n".utf8))
        exit(1)
    }
    bytes = png
}
try bytes.write(to: URL(fileURLWithPath: output))
